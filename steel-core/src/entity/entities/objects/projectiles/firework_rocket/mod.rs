//! Vanilla `FireworkRocketEntity`.
//!
//! The server owns rocket movement, collisions, elytra boosting, lifetime and
//! explosion damage; clients draw the trail and explosion from the synced
//! item and entity event 17.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::vanilla_components::FIREWORKS;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::FireworkRocketEntityData;
use steel_registry::{sound_events, vanilla_damage_types, vanilla_game_events, vanilla_items};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::component::definition::Definition;
use crate::entity::component::entries::{OnHitBlockCtx, OnHitEntityCtx};
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::damage::DamageSource;
use crate::entity::prelude::*;
use crate::entity::projectile::{
    self, ProjectileBase, ProjectileHit, ProjectileProps, triangle_random,
};
use crate::entity::{
    EntityBase, EntityProps, InsideBlockEffectCollector, RemovalReason, SharedEntity,
};
use crate::physics::MoverType;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

const INITIAL_HORIZONTAL_DEVIATION: f64 = 0.002_297;
const EXPLOSION_RADIUS: f64 = 5.0;

#[derive(Default)]
struct Flight {
    life: i32,
    lifetime: i32,
    attached_to_entity: Option<Weak<dyn Entity>>,
}

/// A launched firework rocket.
#[entity_behavior(class = "FireworkRocketEntity")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_rocket, load = Self::load_rocket)]
#[entity(key = "steel:entity/firework_rocket", constructors, definition = Self::define)]
pub struct FireworkRocketEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    flight: CheckedMutex<Flight>,
    #[entity(synced)]
    entity_data: CheckedMutex<FireworkRocketEntityData>,
}

impl FireworkRocketEntity {
    const TICK: StageId = StageId("firework_rocket.tick");
    const ON_HIT_ENTITY: StageId = StageId("firework_rocket.on_hit_entity");
    const ON_HIT_BLOCK: StageId = StageId("firework_rocket.on_hit_block");

    fn define() -> Definition<Self> {
        let definition = projectile::definition();
        Definition {
            on_hit_entity: definition.on_hit_entity.then([Stage::new(
                Self::ON_HIT_ENTITY,
                |rocket: &Self, ctx: &mut OnHitEntityCtx<'_>| {
                    if let Some(world) = rocket.level() {
                        rocket.explode(ctx.entity, &world);
                    }
                    Flow::Continue
                },
            )]),
            on_hit_block: definition.on_hit_block.before(
                projectile::ON_HIT_BLOCK,
                [Stage::new(
                    Self::ON_HIT_BLOCK,
                    |rocket: &Self, ctx: &mut OnHitBlockCtx<'_>| {
                        rocket.on_hit_block_before_super(ctx.entity, ctx.hit);
                        Flow::Continue
                    },
                )],
            ),
            tick: definition.tick.then([Stage::new(
                Self::TICK,
                |rocket: &Self, entity: &mut &SharedEntity| {
                    rocket.entity_tick(entity);
                    Flow::Continue
                },
            )]),
            ..definition
        }
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        attackable: |_| false,
        ..projectile::entity_props()
    };
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        horizontal_hurt_knockback_direction: |rocket, hurt_entity, _| {
            let delta = hurt_entity.position() - rocket.position();
            (delta.x, delta.z)
        },
        ..ProjectileProps::new()
    };

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = FireworkRocketEntityData::new();
        entity_data
            .id_fireworks_item
            .set(ItemStack::new(&vanilla_items::FIREWORK_ROCKET));
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(&Self::PROJECTILE),
            flight: CheckedMutex::new(Flight::default()),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// A rocket launched from `position`, flying for as long as `source_item`
    /// says.
    #[must_use]
    pub fn launched(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        world: Weak<World>,
        source_item: ItemStack,
    ) -> Self {
        let rocket = Self::new(entity_type, id, position, world);
        let flight_count = source_item
            .get(FIREWORKS)
            .map_or(1, |fireworks| 1 + fireworks.flight_duration());
        rocket.entity_data.lock().id_fireworks_item.set(source_item);
        rocket.set_velocity(DVec3::new(
            triangle_random(0.0, INITIAL_HORIZONTAL_DEVIATION),
            0.05,
            triangle_random(0.0, INITIAL_HORIZONTAL_DEVIATION),
        ));
        rocket.flight.lock().lifetime =
            10 * flight_count + rand::random_range(0..6) + rand::random_range(0..7);
        rocket
    }

    /// A rocket stuck to `attached_to`, owned by it; it boosts it while it
    /// glides.
    #[must_use]
    pub fn attached_to_living(
        entity_type: EntityTypeRef,
        id: i32,
        world: Weak<World>,
        source_item: ItemStack,
        attached_to: &SharedEntity,
    ) -> Self {
        let rocket = Self::launched(entity_type, id, attached_to.position(), world, source_item);
        rocket.projectile().set_owner(Some(attached_to));
        rocket.flight.lock().attached_to_entity = Some(Arc::downgrade(attached_to));
        rocket
            .entity_data
            .lock()
            .attached_to_target
            .set(u32::try_from(attached_to.id()).ok());
        rocket
    }

    /// The item the rocket was made from.
    #[must_use]
    pub fn item(&self) -> ItemStack {
        self.entity_data.lock().id_fireworks_item.get().clone()
    }

    /// Sets whether this rocket was fired at an angle (from a crossbow or
    /// dispenser), which turns off its acceleration.
    pub fn set_shot_at_angle(&self, shot_at_angle: bool) {
        self.entity_data.lock().shot_at_angle.set(shot_at_angle);
    }

    /// Whether this rocket was fired at an angle.
    #[must_use]
    pub fn is_shot_at_angle(&self) -> bool {
        *self.entity_data.lock().shot_at_angle.get()
    }

    fn is_attached_to_entity(&self) -> bool {
        self.entity_data.lock().attached_to_target.get().is_some()
    }

    /// The entity this rocket is stuck to: the one it was attached to, or the
    /// living entity with the synced id once (after loading).
    fn attached_entity(&self, world: &World) -> Option<SharedEntity> {
        let attached = self.flight.lock().attached_to_entity.clone();
        if let Some(attached) = attached {
            // A resolved attachment never rebinds through a reused id.
            return attached.upgrade();
        }
        let attached_id = (*self.entity_data.lock().attached_to_target.get())?;
        let attached = world.get_entity_by_id(i32::try_from(attached_id).ok()?)?;
        attached.as_living_entity()?;
        self.flight.lock().attached_to_entity = Some(Arc::downgrade(&attached));
        Some(attached)
    }

    /// `FireworkRocketEntity.tick` after `super.tick()`.
    fn entity_tick(&self, entity: &SharedEntity) {
        let projectile = self.projectile();
        let Some(world) = self.level() else {
            return;
        };

        let hit = if self.is_attached_to_entity() {
            self.tick_attached(&world)
        } else {
            self.tick_free_flying(entity)
        };
        if !self.no_physics()
            && self.is_alive()
            && let Some(hit) = &hit
        {
            projectile.hit_target_or_deflect_self(entity, hit);
            self.mark_velocity_sync();
        }

        projectile.update_rotation();
        let (launched, expired) = {
            let mut flight = self.flight.lock();
            let launched = flight.life == 0;
            flight.life = flight.life.wrapping_add(1);
            (launched, flight.life > flight.lifetime)
        };
        if launched && !self.is_silent() {
            world.play_sound_at(
                &sound_events::ENTITY_FIREWORK_ROCKET_LAUNCH,
                SoundSource::Ambient,
                self.position(),
                3.0,
                1.0,
                None,
            );
        }
        if expired {
            self.explode(entity, &world);
        }
    }

    fn tick_attached(&self, world: &World) -> Option<ProjectileHit> {
        if let Some(attached) = self.attached_entity(world)
            && let Some(living) = attached.as_living_entity()
        {
            let hand_offset = if living.is_fall_flying() {
                living.set_velocity(elytra_boosted_velocity(
                    living.velocity(),
                    living.look_angle(),
                ));
                living.hand_holding_item_angle(&vanilla_items::FIREWORK_ROCKET)
            } else {
                DVec3::ZERO
            };
            if let Err(error) = self.try_set_position(living.position() + hand_offset) {
                log::debug!("failed to move attached firework rocket: {error}");
            }
            self.set_velocity(living.velocity());
        }
        self.projectile().hit_result_on_move_vector()
    }

    fn tick_free_flying(&self, entity: &SharedEntity) -> Option<ProjectileHit> {
        if !self.is_shot_at_angle() {
            let horizontal_acceleration = if self.horizontal_collision() {
                1.0
            } else {
                1.15
            };
            let movement = self.velocity();
            self.set_velocity(DVec3::new(
                movement.x * horizontal_acceleration,
                movement.y + 0.04,
                movement.z * horizontal_acceleration,
            ));
        }
        let movement = self.velocity();
        let hit = self.projectile().hit_result_on_move_vector();
        Arc::clone(entity).move_entity(MoverType::SelfMovement, movement);
        self.apply_effects_from_blocks();
        self.set_velocity(movement);
        hit
    }

    fn explosion_count(&self) -> usize {
        self.entity_data
            .lock()
            .id_fireworks_item
            .get()
            .get(FIREWORKS)
            .map_or(0, |fireworks| fireworks.explosions().len())
    }

    fn fireworks_damage(&self, entity: &SharedEntity) -> DamageSource {
        let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS)
            .with_direct_entity(Arc::clone(entity));
        match self.projectile().owner() {
            Some(owner) => source.with_causing_entity(owner),
            None => source,
        }
    }

    /// `FireworkRocketEntity.dealExplosionDamage`: the attached entity and
    /// living entities in view within five blocks take damage.
    fn deal_explosion_damage(&self, entity: &SharedEntity, world: &Arc<World>) {
        let explosion_count = self.explosion_count();
        if explosion_count == 0 {
            return;
        }
        let damage = 5.0 + explosion_count as f32 * 2.0;
        let attached = self.attached_entity(world);
        if let Some(attached) = &attached {
            attached.hurt(world, &self.fireworks_damage(entity), damage);
        }

        let rocket_position = self.position();
        let search_box = self.bounding_box().inflate(EXPLOSION_RADIUS);
        for target in world.get_entities_in_aabb_matching(&search_box, Entity::is_living_entity) {
            if attached
                .as_ref()
                .is_some_and(|attached| Arc::ptr_eq(attached, &target))
            {
                continue;
            }
            let distance_squared = rocket_position.distance_squared(target.position());
            if distance_squared > EXPLOSION_RADIUS * EXPLOSION_RADIUS {
                continue;
            }
            let target_height = f64::from(target.base().dimensions().height);
            let can_see = [0.0, 0.5].into_iter().any(|height_scale| {
                let to = target.position() + DVec3::new(0.0, target_height * height_scale, 0.0);
                world
                    .clip(
                        rocket_position,
                        to,
                        ClipBlockShape::Collider,
                        ClipFluid::None,
                    )
                    .is_miss()
            });
            if !can_see {
                continue;
            }
            let distance_scale =
                ((EXPLOSION_RADIUS - distance_squared.sqrt()) / EXPLOSION_RADIUS).sqrt();
            target.hurt(
                world,
                &self.fireworks_damage(entity),
                damage * distance_scale as f32,
            );
        }
    }

    fn explode(&self, entity: &SharedEntity, world: &Arc<World>) {
        self.broadcast_entity_event(EntityStatus::FireworksExplode);
        let owner = self.projectile().owner();
        self.game_event_with_source_entity(&vanilla_game_events::EXPLODE, owner.as_deref());
        self.deal_explosion_damage(entity, world);
        self.set_removed(RemovalReason::Discarded);
    }

    /// `FireworkRocketEntity.onHitBlock`: the block reacts as if the rocket
    /// were inside it, and a rocket with explosions explodes.
    /// `FireworkRocketEntity.onHitBlock` before `super`.
    fn on_hit_block_before_super(&self, entity: &SharedEntity, hit: &ClipHitResult) {
        if let Some(world) = self.level() {
            let state = world.get_block_state(hit.block_pos);
            let mut ignored_effects = InsideBlockEffectCollector::new();
            BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .entity_inside(
                    state,
                    &world,
                    hit.block_pos,
                    self,
                    &mut ignored_effects,
                    true,
                );
            if self.explosion_count() != 0 {
                self.explode(entity, &world);
            }
        }
    }

    fn save_rocket(&self, nbt: &mut NbtCompound) {
        {
            let flight = self.flight.lock();
            nbt.insert("Life", flight.life);
            nbt.insert("LifeTime", flight.lifetime);
        }
        nbt.insert("FireworksItem", self.item().to_nbt_tag_ref());
        nbt.insert("ShotAtAngle", i8::from(self.is_shot_at_angle()));
    }

    fn load_rocket(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        {
            let mut flight = self.flight.lock();
            flight.life = nbt.int("Life").unwrap_or(0);
            flight.lifetime = nbt.int("LifeTime").unwrap_or(0);
        }
        let item = nbt
            .compound("FireworksItem")
            .and_then(|item| ItemStack::from_borrowed_compound(&item))
            .unwrap_or_else(|| ItemStack::new(&vanilla_items::FIREWORK_ROCKET));
        self.entity_data.lock().id_fireworks_item.set(item);
        self.set_shot_at_angle(nbt.byte("ShotAtAngle").is_some_and(|value| value != 0));
    }
}

/// The velocity a rocket gives a gliding entity looking along `look_angle`.
fn elytra_boosted_velocity(movement: DVec3, look_angle: DVec3) -> DVec3 {
    movement + look_angle * 0.1 + (look_angle * 1.5 - movement) * 0.5
}

#[cfg(test)]
mod tests;
