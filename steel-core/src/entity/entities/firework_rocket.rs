//! Firework rocket projectile entity (`FireworkRocketEntity`).
//!
//! The server owns rocket movement, collision, Elytra boosting, lifetime,
//! explosion damage, and entity-event dispatch. Firework trail and explosion
//! particles are created by the client from synced rocket data and event 17.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::{entity_behavior, entity_impl};
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::vanilla_components::FIREWORKS;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::FireworkRocketEntityData;
use steel_registry::{
    sound_events, vanilla_damage_type_tags, vanilla_damage_types, vanilla_game_events,
    vanilla_items,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityEventSource, EntitySyncedData,
    InsideBlockEffectCollector, LivingEntity, Projectile, ProjectileBase, ProjectileHit,
    RemovalReason, SharedEntity,
};
use crate::physics::MoverType;
use crate::world::game_event_context::GameEventContext;
use crate::world::{ClipBlockShape, ClipFluid, ClipHitResult, World};

const INITIAL_VERTICAL_VELOCITY: f64 = 0.05;
const INITIAL_HORIZONTAL_DEVIATION: f64 = 0.002_297;
const HORIZONTAL_ACCELERATION: f64 = 1.15;
const VERTICAL_ACCELERATION: f64 = 0.04;
const ELYTRA_TARGET_SPEED: f64 = 1.5;
const ELYTRA_POWER_ADD: f64 = 0.1;
const ELYTRA_VELOCITY_BLEND: f64 = 0.5;
const EXPLOSION_RADIUS: f64 = 5.0;
const EXPLOSION_RADIUS_SQUARED: f64 = EXPLOSION_RADIUS * EXPLOSION_RADIUS;

struct FireworkRocketState {
    life: i32,
    lifetime: i32,
    attached_to_entity: Option<Weak<dyn Entity>>,
}

/// A launched firework rocket.
#[entity_behavior(class = "FireworkRocketEntity")]
pub struct FireworkRocketEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<FireworkRocketEntityData>,
    projectile_base: ProjectileBase,
    state: SyncMutex<FireworkRocketState>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `FireworkRocketEntity`.
unsafe impl DowncastType for FireworkRocketEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/firework_rocket");
}

impl FireworkRocketEntity {
    /// Creates an uninitialized rocket for the entity factory.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(FireworkRocketEntityData::new()),
            projectile_base: ProjectileBase::new(),
            state: SyncMutex::new(FireworkRocketState {
                life: 0,
                lifetime: 0,
                attached_to_entity: None,
            }),
        }
    }

    /// Creates a rocket from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(FireworkRocketEntityData::new()),
            projectile_base: ProjectileBase::new(),
            state: SyncMutex::new(FireworkRocketState {
                life: 0,
                lifetime: 0,
                attached_to_entity: None,
            }),
        }
    }

    fn is_base_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable() && !source.bypasses_invulnerability()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
    }

    /// Creates a normally launched rocket at an exact position.
    #[must_use]
    pub fn launched(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        world: Weak<World>,
        source_item: ItemStack,
    ) -> Self {
        let rocket = Self::new(entity_type, id, position, world);
        rocket.initialize_launch(source_item);
        rocket
    }

    /// Creates a rocket attached to a living entity that will be resolved from
    /// its synced runtime ID after the rocket enters the world.
    #[must_use]
    pub fn attached_to_living(
        entity_type: EntityTypeRef,
        id: i32,
        world: Weak<World>,
        source_item: ItemStack,
        attached_to: &dyn LivingEntity,
    ) -> Self {
        let rocket = Self::launched(entity_type, id, attached_to.position(), world, source_item);
        rocket.set_owner_uuid(Some(attached_to.uuid()));
        if let Ok(attached_id) = u32::try_from(attached_to.id()) {
            rocket
                .entity_data
                .lock()
                .attached_to_target
                .set(Some(attached_id));
        }
        rocket
    }

    fn initialize_launch(&self, source_item: ItemStack) {
        let flight_count = source_item
            .get(FIREWORKS)
            .map_or(1, |fireworks| 1 + fireworks.flight_duration());
        self.entity_data.lock().id_fireworks_item.set(source_item);
        self.set_velocity(DVec3::new(
            triangle_random(0.0, INITIAL_HORIZONTAL_DEVIATION),
            INITIAL_VERTICAL_VELOCITY,
            triangle_random(0.0, INITIAL_HORIZONTAL_DEVIATION),
        ));
        self.state.lock().lifetime =
            10 * flight_count + rand::random_range(0..6) + rand::random_range(0..7);
    }

    /// Sets whether this rocket was fired at an angle.
    pub fn set_shot_at_angle(&self, shot_at_angle: bool) {
        self.entity_data.lock().shot_at_angle.set(shot_at_angle);
    }

    /// Returns whether this rocket was fired at an angle.
    #[must_use]
    pub fn is_shot_at_angle(&self) -> bool {
        *self.entity_data.lock().shot_at_angle.get()
    }

    fn is_attached_to_entity(&self) -> bool {
        self.entity_data.lock().attached_to_target.get().is_some()
    }

    fn attached_entity(&self, world: &Arc<World>) -> Option<SharedEntity> {
        if let Some(attached) = self
            .state
            .lock()
            .attached_to_entity
            .as_ref()
            .and_then(Weak::upgrade)
            && !attached.is_removed()
            && attached.as_living_entity().is_some()
        {
            return Some(attached);
        }

        let attached_id = *self.entity_data.lock().attached_to_target.get();
        let attached_id = i32::try_from(attached_id?).ok()?;
        let attached = world.get_entity_by_id(attached_id)?;
        attached.as_living_entity()?;
        self.state.lock().attached_to_entity = Some(Arc::downgrade(&attached));
        Some(attached)
    }

    fn tick_attached(&self, world: &Arc<World>) -> Option<ProjectileHit> {
        if let Some(attached) = self.attached_entity(world)
            && let Some(living) = attached.as_living_entity()
        {
            let hand_angle = if living.is_fall_flying() {
                let look_angle = living.look_angle();
                let movement = living.velocity();
                living.set_velocity(elytra_boosted_velocity(movement, look_angle));
                living.hand_holding_item_angle(&vanilla_items::FIREWORK_ROCKET)
            } else {
                DVec3::ZERO
            };

            if let Err(error) = self.try_set_position(living.position() + hand_angle) {
                log::debug!("failed to move attached firework rocket: {error}");
            }
            self.set_velocity(living.velocity());
        }

        self.get_hit_result_on_move_vector()
    }

    fn tick_free_flying(&self) -> Option<ProjectileHit> {
        if !self.is_shot_at_angle() {
            let horizontal_acceleration = if self.horizontal_collision() {
                1.0
            } else {
                HORIZONTAL_ACCELERATION
            };
            let movement = self.velocity();
            self.set_velocity(DVec3::new(
                movement.x * horizontal_acceleration,
                movement.y + VERTICAL_ACCELERATION,
                movement.z * horizontal_acceleration,
            ));
        }

        let movement = self.velocity();
        let hit = self.get_hit_result_on_move_vector();
        self.move_entity(MoverType::SelfMovement, movement);
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

    fn has_explosion(&self) -> bool {
        self.explosion_count() != 0
    }

    fn fireworks_damage_source(&self) -> DamageSource {
        let mut source = DamageSource::environment(&vanilla_damage_types::FIREWORKS)
            .with_direct_entity(self.id());
        if let Some(owner) = self.get_owner() {
            source = source.with_causing_entity(owner.id());
        }
        source
    }

    fn deal_explosion_damage(&self, world: &Arc<World>) {
        let explosion_count = self.explosion_count();
        if explosion_count == 0 {
            return;
        }
        let damage_amount = 5.0 + explosion_count as f32 * 2.0;
        let attached = self.attached_entity(world);
        let attached_id = attached.as_ref().map(|entity| entity.id());

        if let Some(attached) = &attached {
            attached.hurt(world, &self.fireworks_damage_source(), damage_amount);
        }

        let rocket_position = self.position();
        let search_box = self.bounding_box().inflate(EXPLOSION_RADIUS);
        for target in
            world.get_entities_in_aabb_matching(&search_box, |entity| entity.is_living_entity())
        {
            if attached_id == Some(target.id()) {
                continue;
            }
            let distance_squared = rocket_position.distance_squared(target.position());
            if distance_squared > EXPLOSION_RADIUS_SQUARED {
                continue;
            }

            let target_height = f64::from(target.base().dimensions().height);
            let can_see = [0.0, 0.5].into_iter().any(|height_scale| {
                let target_position = target.position();
                let to = DVec3::new(
                    target_position.x,
                    target_position.y + target_height * height_scale,
                    target_position.z,
                );
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

            let distance = distance_squared.sqrt();
            let distance_scale = ((EXPLOSION_RADIUS - distance) / EXPLOSION_RADIUS).sqrt();
            target.hurt(
                world,
                &self.fireworks_damage_source(),
                damage_amount * distance_scale as f32,
            );
        }
    }

    fn explode(&self, world: &Arc<World>) {
        self.broadcast_entity_event(EntityStatus::FireworksExplode);
        let owner = self.get_owner();
        world.game_event_at(
            &vanilla_game_events::EXPLODE,
            self.position(),
            &GameEventContext::new(owner.as_deref(), None),
        );
        self.deal_explosion_damage(world);
        self.set_removed(RemovalReason::Discarded);
    }

    fn run_hit_block_entity_inside(&self, world: &Arc<World>, hit: &ClipHitResult) {
        let state = world.get_block_state(hit.block_pos);
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let mut ignored_effects = InsideBlockEffectCollector::new();
        behavior.entity_inside(
            state,
            world,
            hit.block_pos,
            self.as_entity_event_source(),
            &mut ignored_effects,
            true,
        );
    }

    #[cfg(test)]
    fn life_and_lifetime(&self) -> (i32, i32) {
        let state = self.state.lock();
        (state.life, state.lifetime)
    }
}

#[entity_impl(class(projectile))]
impl Entity for FireworkRocketEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.projectile_base_tick();
        let Some(world) = self.level() else {
            return;
        };

        let hit = if self.is_attached_to_entity() {
            self.tick_attached(&world)
        } else {
            self.tick_free_flying()
        };
        if !self.no_physics()
            && self.is_alive()
            && let Some(hit) = &hit
        {
            self.hit_target_or_deflect_self(hit);
            self.mark_velocity_sync();
        }

        self.update_rotation();
        let (play_launch_sound, expired) = {
            let mut state = self.state.lock();
            let play_launch_sound = state.life == 0;
            state.life = state.life.wrapping_add(1);
            (play_launch_sound, state.life > state.lifetime)
        };
        if play_launch_sound && !self.is_silent() {
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
            self.explode(&world);
        }
    }

    fn spawn_data(&self) -> i32 {
        self.get_owner().map_or(0, |owner| owner.id())
    }

    fn restore_owner_reference(&self, owner: &SharedEntity) {
        self.cache_owner_entity(owner);
    }

    fn projectile_owner_uuid(&self) -> Option<uuid::Uuid> {
        self.owner_uuid()
    }

    fn projectile_owner(&self) -> Option<SharedEntity> {
        self.get_owner()
    }

    fn attackable(&self) -> bool {
        false
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn hurt(&self, _world: &World, source: &DamageSource, _amount: f32) -> bool {
        if !self.is_base_invulnerable_to(source) {
            self.mark_hurt();
        }
        false
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        let state = self.state.lock();
        nbt.insert("Life", state.life);
        nbt.insert("LifeTime", state.lifetime);
        drop(state);
        nbt.insert("FireworksItem", self.get_item().to_nbt_tag_ref());
        nbt.insert("ShotAtAngle", i8::from(self.is_shot_at_angle()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        {
            let mut state = self.state.lock();
            state.life = nbt.int("Life").unwrap_or(0);
            state.lifetime = nbt.int("LifeTime").unwrap_or(0);
        }
        let item = nbt
            .compound("FireworksItem")
            .and_then(|item| ItemStack::from_borrowed_compound(&item))
            .unwrap_or_else(|| ItemStack::new(&vanilla_items::FIREWORK_ROCKET));
        self.set_item(item);
        self.set_shot_at_angle(nbt.byte("ShotAtAngle").is_some_and(|value| value != 0));
    }
}

impl Projectile for FireworkRocketEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    fn calculate_horizontal_hurt_knockback_direction(
        &self,
        hurt_entity: &dyn LivingEntity,
        _damage_source: &DamageSource,
    ) -> (f64, f64) {
        let delta = hurt_entity.position() - self.position();
        (delta.x, delta.z)
    }

    fn on_hit_entity(&self, _entity: &SharedEntity, _location: DVec3) {
        if let Some(world) = self.level() {
            self.explode(&world);
        }
    }

    fn on_hit_block(&self, hit: &ClipHitResult) {
        if let Some(world) = self.level() {
            self.run_hit_block_entity_inside(&world, hit);
            if self.has_explosion() {
                self.explode(&world);
            }
        }
        self.projectile_on_hit_block(hit);
    }
}

impl FireworkRocketEntity {
    /// Returns the synced source stack rendered by the client.
    #[must_use]
    pub fn get_item(&self) -> ItemStack {
        self.entity_data.lock().id_fireworks_item.get().clone()
    }

    /// Replaces the synced source stack.
    pub fn set_item(&self, item: ItemStack) {
        self.entity_data.lock().id_fireworks_item.set(item);
    }
}

fn triangle_random(mode: f64, deviation: f64) -> f64 {
    mode + deviation * (rand::random::<f64>() - rand::random::<f64>())
}

fn elytra_boosted_velocity(movement: DVec3, look_angle: DVec3) -> DVec3 {
    movement
        + look_angle * ELYTRA_POWER_ADD
        + (look_angle * ELYTRA_TARGET_SPEED - movement) * ELYTRA_VELOCITY_BLEND
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::data_components::components::Fireworks;
    use steel_registry::data_components::vanilla_components::FIREWORKS;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{test_support::init_test_registry, vanilla_entities, vanilla_items};

    use crate::{
        entity::{Entity, Projectile, entities::PigEntity},
        test_support::test_world,
    };

    use super::*;

    #[test]
    fn launched_rocket_uses_fireworks_flight_duration_for_lifetime() {
        init_test_registry();
        let mut item = ItemStack::new(&vanilla_items::FIREWORK_ROCKET);
        item.set(
            FIREWORKS,
            Fireworks::new(3, Vec::new()).unwrap_or_else(|error| {
                panic!("valid firework component should construct: {error}")
            }),
        );
        let rocket = FireworkRocketEntity::launched(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
            item,
        );

        let (_, lifetime) = rocket.life_and_lifetime();
        assert!((40..=51).contains(&lifetime));
        assert_eq!(
            rocket.velocity().y.to_bits(),
            INITIAL_VERTICAL_VELOCITY.to_bits()
        );
    }

    #[test]
    fn firework_uses_vanilla_neutral_sound_source() {
        init_test_registry();
        let rocket = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
        );

        assert_eq!(rocket.sound_source(), SoundSource::Neutral);
    }

    #[test]
    fn hurt_marks_rocket_unless_base_invulnerable_and_always_returns_false() {
        init_test_registry();
        let rocket = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(!rocket.hurt(test_world(), &source, 1.0));
        assert!(rocket.hurt_marked());

        rocket.clear_hurt_mark();
        rocket.set_invulnerable(true);
        assert!(!rocket.hurt(test_world(), &source, 1.0));
        assert!(!rocket.hurt_marked());
    }

    #[test]
    fn firework_metadata_carries_item_attachment_and_angle() {
        init_test_registry();
        let target: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            19,
            DVec3::new(1.0, 2.0, 3.0),
            Weak::new(),
        ));
        let Some(living_target) = target.as_living_entity() else {
            panic!("pig test entity should be living");
        };
        let rocket = FireworkRocketEntity::attached_to_living(
            &vanilla_entities::FIREWORK_ROCKET,
            2,
            Weak::new(),
            ItemStack::new(&vanilla_items::FIREWORK_ROCKET),
            living_target,
        );
        rocket.set_shot_at_angle(true);

        let data = rocket.entity_data.lock();
        assert_eq!(*data.attached_to_target.get(), Some(19));
        assert!(*data.shot_at_angle.get());
        assert!(
            data.id_fireworks_item
                .get()
                .is(&vanilla_items::FIREWORK_ROCKET)
        );
        assert_eq!(rocket.owner_uuid(), Some(target.uuid()));
    }

    #[test]
    fn firework_state_persists_with_vanilla_keys() {
        init_test_registry();
        let rocket = FireworkRocketEntity::launched(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::ZERO,
            Weak::new(),
            ItemStack::new(&vanilla_items::FIREWORK_ROCKET),
        );
        {
            let mut state = rocket.state.lock();
            state.life = 7;
            state.lifetime = 29;
        }
        rocket.set_shot_at_angle(true);
        rocket.set_owner_uuid(Some(uuid::Uuid::from_u128(42)));

        let mut nbt = NbtCompound::new();
        rocket.save_additional(&mut nbt);
        assert_eq!(nbt.int("Life"), Some(7));
        assert_eq!(nbt.int("LifeTime"), Some(29));
        assert_eq!(nbt.byte("ShotAtAngle"), Some(1));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test NBT should reborrow: {error}"));
        let loaded = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        loaded.load_additional((&borrowed).into());

        assert_eq!(loaded.life_and_lifetime(), (7, 29));
        assert!(loaded.is_shot_at_angle());
        assert_eq!(loaded.owner_uuid(), Some(uuid::Uuid::from_u128(42)));
        assert!(loaded.get_item().is(&vanilla_items::FIREWORK_ROCKET));
    }

    #[test]
    fn firework_knockback_direction_points_from_rocket_to_target() {
        init_test_registry();
        let rocket = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            1,
            DVec3::new(2.0, 0.0, 3.0),
            Weak::new(),
        );
        let target = PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(5.0, 0.0, 1.0),
            Weak::new(),
        );
        let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);

        assert_eq!(
            rocket.calculate_horizontal_hurt_knockback_direction(&target, &source),
            (3.0, -2.0)
        );
        assert!(rocket.as_projectile().is_some());
    }

    #[test]
    fn firework_damage_source_has_no_raw_position() {
        init_test_registry();
        let rocket = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            23,
            DVec3::new(1.0, 2.0, 3.0),
            Weak::new(),
        );

        let source = rocket.fireworks_damage_source();

        assert_eq!(source.direct_entity_id, Some(23));
        assert!(source.source_position.is_none());
    }

    #[test]
    fn elytra_boost_matches_vanilla_vector_formula() {
        let movement = DVec3::new(0.2, -0.1, 0.4);
        let look_angle = DVec3::new(0.0, 0.0, 1.0);

        assert_eq!(
            elytra_boosted_velocity(movement, look_angle),
            DVec3::new(0.1, -0.05, 1.05)
        );
    }
}
