//! Vanilla `FishingHook`: the bobber a fishing rod casts.
//!
//! The owning player keeps a weak reference to its hook, cleared when the
//! hook is removed.

use std::f32::consts::PI;
use std::sync::Arc;

use glam::DVec3;
use rand::{RngExt, rng};
use steel_macros::entity_behavior;
use steel_math::{DEG_TO_RAD, DEGREE_360, trig};
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::LootContext;
use steel_registry::particle_type::ParticleData;
use steel_registry::vanilla_entity_data::FishingBobberEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_particle_types::{BUBBLE, FISHING, SPLASH};
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_custom_stats, vanilla_entities, vanilla_items,
    vanilla_loot_tables,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Downcast as _};

use crate::entity::entities::{ExperienceOrbEntity, ItemEntity};
use crate::entity::prelude::*;
use crate::entity::projectile::{self, ProjectileBase, ProjectileProps, triangle_random};
use crate::entity::{
    EntityBase, EntityMovementEmission, EntityProps, RemovalReason, SharedEntity, entity_loot_ref,
    next_entity_id,
};
use crate::fluid::get_height;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Ticks a bobbing hook may spend out of water before it stops counting as
/// open water.
pub const MAX_OUT_OF_WATER_TIME: i32 = 10;
const MAX_DISTANCE_SQR: f64 = 32.0 * 32.0;

const DMG_DEFAULT: i32 = 5;
const DMG_ITEM_ENTITY: i32 = 3;
const DMG_ON_GROUND: i32 = 2;
const DMG_CAUGHT: i32 = 1;

const ONE_SECOND: i32 = 20;
const TWO_SECONDS: i32 = 40;
const THREE_SECONDS: i32 = 60;
const FOUR_SECONDS: i32 = 80;
const FIVE_SECONDS: i32 = 100;
const THIRTY_SECONDS: i32 = 600;
const ONE_MINUTE: i32 = 1200;

/// A fishing hook.
#[entity_behavior(class = "FishingHook")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, no_save_data)]
#[entity(key = "steel:entity/fishing_hook", constructors)]
pub struct FishingHookEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    projectile: ProjectileBase,
    hook_state: CheckedMutex<FishingHookState>,
    synchronized_random: CheckedMutex<LegacyRandom>,
    #[entity(synced)]
    entity_data: CheckedMutex<FishingBobberEntityData>,
}

/// The fishing state of one hook.
pub struct FishingHookState {
    out_of_water_time: i32,
    life: i32,
    nibble: i32,
    time_until_lured: i32,
    time_until_hooked: i32,
    fish_angle: f32,
    open_water: bool,
    /// Vanilla `currentState`.
    bobber_state: BobberState,
    hooked_entity: Option<SharedEntity>,
    luck: i32,
    lure_speed: i32,
}

impl FishingHookState {
    /// A flying hook with the rod's `lure_speed` and `luck` bonuses.
    #[must_use]
    pub fn new(lure_speed: i32, luck: i32) -> Self {
        Self {
            out_of_water_time: 0,
            life: 0,
            nibble: 0,
            time_until_lured: 0,
            time_until_hooked: 0,
            fish_angle: 0.0,
            open_water: false,
            bobber_state: BobberState::Flying,
            hooked_entity: None,
            luck: luck.max(0),
            lure_speed: lure_speed.max(0),
        }
    }
}

impl FishingHookEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        tick: |hook, entity| hook.entity_tick(entity),
        // Vanilla `FishingHook.remove`: the player stops fishing with it.
        on_removal: |hook, _| {
            if let Some(owner) = hook.projectile().owner()
                && let Some(player) = owner.as_player()
            {
                player.clear_fishing_hook(hook);
            }
        },
        movement_emission: |_| EntityMovementEmission::None,
        can_use_portal: |_, _| false,
        // The client needs the owner to draw the line; an ownerless hook
        // sends its own id.
        spawn_data: |hook| {
            hook.projectile()
                .owner()
                .map_or(hook.id(), |owner| owner.id())
        },
        ..projectile::entity_props()
    };
    const PROJECTILE: ProjectileProps<Self> = ProjectileProps {
        can_hit_entity: |hook, target| {
            hook.projectile().can_hit_entity_default(target)
                || (target.is_alive() && target.is::<ItemEntity>())
        },
        on_hit_entity: |hook, _, target, _| hook.set_hooked_entity(Some(Arc::clone(target))),
        ..ProjectileProps::new()
    };

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            projectile: ProjectileBase::new(&Self::PROJECTILE),
            hook_state: CheckedMutex::new(FishingHookState::new(0, 0)),
            synchronized_random: CheckedMutex::new(LegacyRandom::from_seed(0)),
            entity_data: CheckedMutex::new(FishingBobberEntityData::new()),
        }
    }

    /// Vanilla `FishingHook(Player, Level, int, int)`: casts the hook from
    /// `player`'s eyes along their look.
    pub fn shoot_from_player(self: &Arc<Self>, player: &Arc<Player>, luck: i32, lure_speed: i32) {
        const MAGIC_OFFSET: f64 = 0.010_336_5;

        {
            let mut state = self.hook_state.lock();
            state.luck = luck.max(0);
            state.lure_speed = lure_speed.max(0);
        }

        let (yaw, pitch) = player.rotation();
        let owner: SharedEntity = Arc::<Player>::clone(player);
        self.set_owner(&owner);

        let y_cos = trig::cos(-yaw * DEG_TO_RAD - PI);
        let y_sin = trig::sin(-yaw * DEG_TO_RAD - PI);
        let x_cos = -trig::cos(-pitch * DEG_TO_RAD);
        let x_sin = trig::sin(-pitch * DEG_TO_RAD);

        let x = player.position().x - f64::from(y_sin) * 0.3;
        let y = player.eye_y();
        let z = player.position().z - f64::from(y_cos) * 0.3;
        self.snap_to(DVec3::new(x, y, z), yaw, pitch);

        let clamped_y = f64::from((-(x_sin / x_cos)).clamp(-5.0, 5.0));
        let mut movement = DVec3::new(-f64::from(y_sin), clamped_y, -f64::from(y_cos));
        let distance = movement.length();
        movement *= DVec3::new(
            0.6 / distance + triangle_random(0.5, MAGIC_OFFSET),
            0.6 / distance + triangle_random(0.5, MAGIC_OFFSET),
            0.6 / distance + triangle_random(0.5, MAGIC_OFFSET),
        );
        self.set_velocity(movement);

        let horizontal_distance = (movement.x * movement.x + movement.z * movement.z).sqrt();
        self.set_rotation((
            movement.x.atan2(movement.z).to_degrees() as f32,
            movement.y.atan2(horizontal_distance).to_degrees() as f32,
        ));
        self.base().set_old_rotation_to_current();
    }

    /// `FishingHook.setOwner`: a player owner fishes with this hook.
    pub fn set_owner(self: &Arc<Self>, owner: &SharedEntity) {
        self.projectile().set_owner(Some(owner));
        if let Some(player) = owner.as_player() {
            player.set_fishing_hook(self);
        }
    }

    /// The owner, when it is a player.
    fn player_owner(&self) -> Option<SharedEntity> {
        self.projectile()
            .owner()
            .filter(|owner| owner.as_player().is_some())
    }

    /// `FishingHook.shouldStopFishing`: discards the hook unless its player
    /// can act, holds a rod and is within 32 blocks.
    fn should_stop_fishing(&self, owner: &Player) -> bool {
        let holds_rod = {
            let inventory = owner.inventory.lock();
            inventory
                .get_item_in_hand(InteractionHand::MainHand)
                .is(&vanilla_items::FISHING_ROD)
                || inventory.get_offhand_item().is(&vanilla_items::FISHING_ROD)
        };
        if owner.can_interact_with_level()
            && holds_rod
            && self.distance_to_sqr(owner.position()) <= MAX_DISTANCE_SQR
        {
            return false;
        }
        self.set_removed(RemovalReason::Discarded);
        true
    }

    /// Stores the hooked entity, synced as its id plus one (zero for none).
    fn set_hooked_entity(&self, hooked: Option<SharedEntity>) {
        let hooked_entity_id = hooked.as_ref().map_or(0, |entity| entity.id() + 1);
        self.hook_state.lock().hooked_entity = hooked;
        self.entity_data
            .lock()
            .fishing_hook
            .hooked_entity
            .set(hooked_entity_id);
    }

    /// Runs catching fish logic
    #[expect(
        clippy::too_many_lines,
        clippy::similar_names,
        reason = "Logic that belongs together is being kept together + X and Z movement components intentionally have similar names"
    )]
    fn catching_fish(&self, pos: BlockPos, state: &mut FishingHookState) {
        const RAINING_BONUS_PROBABILITY: f64 = 0.25;
        const SKY_OBSTRUCTION_NERF_PROBABILITY: f64 = 0.5;

        let mut fishing_speed = 1;
        let above = pos.above();

        let Some(world) = self.level() else {
            return;
        };

        if rng().random::<f64>() < RAINING_BONUS_PROBABILITY && world.is_raining_at(above) {
            fishing_speed += 1;
        }

        if rng().random::<f64>() < SKY_OBSTRUCTION_NERF_PROBABILITY && !world.can_see_sky(above) {
            fishing_speed -= 1;
        }

        if state.nibble > 0 {
            state.nibble -= 1;

            if state.nibble <= 0 {
                state.time_until_lured = 0;
                state.time_until_hooked = 0;
                self.entity_data.lock().fishing_hook.biting.set(false);
            }
        } else if state.time_until_hooked > 0 {
            state.time_until_hooked -= fishing_speed;

            if state.time_until_hooked > 0 {
                state.fish_angle += triangle_random(0.0, 9.188) as f32;

                let angle = state.fish_angle * DEG_TO_RAD;
                let angle_sin = trig::sin(angle);
                let angle_cos = trig::cos(angle);

                let fish_x = self.position().x
                    + f64::from(angle_sin) * f64::from(state.time_until_hooked) * 0.1;
                let fish_y = self.position().y.floor() + 1.0;
                let fish_z = self.position().z
                    + f64::from(angle_cos) * f64::from(state.time_until_hooked) * 0.1;

                let Some(world) = self.level() else {
                    return;
                };

                let splash_block_state =
                    world.get_block_state(BlockPos::containing(fish_x, fish_y - 1.0, fish_z));

                if splash_block_state.get_block() == &vanilla_blocks::WATER {
                    const PARTICLE_SPAWN_PROBABILITY: f32 = 0.15;
                    if rng().random::<f32>() < PARTICLE_SPAWN_PROBABILITY {
                        world.send_particles(
                            ParticleData::simple(&BUBBLE),
                            DVec3::new(fish_x, fish_y - 0.1, fish_z),
                            1,
                            DVec3::new(angle_sin.into(), 0.1, angle_cos.into()),
                            0.0,
                        );
                    }

                    let particle_x_mov = angle_sin * 0.04;
                    let particle_z_mov = angle_cos * 0.04;

                    // Yes, according to the src, x and z are swapped in the second `DVec3`
                    world.send_particles(
                        ParticleData::simple(&FISHING),
                        DVec3::new(fish_x, fish_y, fish_z),
                        0,
                        DVec3::new(particle_z_mov.into(), 0.01, -f64::from(particle_x_mov)),
                        1.0,
                    );
                    world.send_particles(
                        ParticleData::simple(&FISHING),
                        DVec3::new(fish_x, fish_y, fish_z),
                        0,
                        DVec3::new(-f64::from(particle_z_mov), 0.01, particle_x_mov.into()),
                        1.0,
                    );
                }
            } else {
                let Some(world) = self.level() else {
                    return;
                };

                self.play_sound(
                    &sound_events::ENTITY_FISHING_BOBBER_SPLASH,
                    0.25,
                    1.0 + (rng().random::<f32>() - rng().random::<f32>()) * 0.4,
                );

                let bb_width = self.bounding_box().width();
                let y = self.position().y + 0.5;
                let particle_pos = DVec3::new(self.position().x, y, self.position().z);
                let particle_count = (1.0 + bb_width * 20.0) as i32;
                let particle_spread = DVec3::new(bb_width, 0.0, bb_width);

                world.send_particles(
                    ParticleData::simple(&BUBBLE),
                    particle_pos,
                    particle_count,
                    particle_spread,
                    0.2,
                );
                world.send_particles(
                    ParticleData::simple(&FISHING),
                    particle_pos,
                    particle_count,
                    particle_spread,
                    0.2,
                );

                state.nibble = rng().random_range(ONE_SECOND..=TWO_SECONDS);
                self.entity_data.lock().fishing_hook.biting.set(true);
            }
        } else if state.time_until_lured > 0 {
            state.time_until_lured -= fishing_speed;
            let mut tease_chance: f32 = 0.15;

            match state.time_until_lured {
                0..ONE_SECOND => {
                    tease_chance += (ONE_SECOND - state.time_until_lured) as f32 * 0.05;
                }
                ONE_SECOND..TWO_SECONDS => {
                    tease_chance += (TWO_SECONDS - state.time_until_lured) as f32 * 0.02;
                }
                TWO_SECONDS..THREE_SECONDS => {
                    tease_chance += (THREE_SECONDS - state.time_until_lured) as f32 * 0.01;
                }
                _ => {}
            }

            if rng().random::<f32>() < tease_chance {
                let Some(world) = self.level() else {
                    return;
                };

                let angle = rng().random_range(0.0..=DEGREE_360) * DEG_TO_RAD;
                let dist = rng().random_range(25.0..=60.0);

                let fish_x = self.position().x + f64::from(trig::sin(angle)) * dist * 0.1;
                let fish_y = self.position().y.floor() + 1.0;
                let fish_z = self.position().z + f64::from(trig::cos(angle)) * dist * 0.1;

                let splash_block_state =
                    world.get_block_state(BlockPos::containing(fish_x, fish_y - 1.0, fish_z));

                if splash_block_state.get_block() == &vanilla_blocks::WATER {
                    world.send_particles(
                        ParticleData::simple(&SPLASH),
                        DVec3::new(fish_x, fish_y, fish_z),
                        2 + rng().random_range(0..=2),
                        DVec3::new(0.1, 0.0, 0.1),
                        0.0,
                    );
                }
            }

            if state.time_until_lured <= 0 {
                state.fish_angle = rng().random_range(0.0..=DEGREE_360);
                state.time_until_hooked = rng().random_range(ONE_SECOND..=FOUR_SECONDS);
            }
        } else {
            state.time_until_lured = rng().random_range(FIVE_SECONDS..=THIRTY_SECONDS);
            state.time_until_lured -= state.lure_speed;
        }
    }

    /// Calculates if the area the player is currently fishing in is open water.
    fn calculate_open_water(&self, pos: BlockPos) -> bool {
        let mut prev_layer = OpenWaterType::Invalid;

        for y in -1..=2 {
            let offset_from = BlockPos::new(pos.x() - 2, pos.y() + y, pos.z() - 2);
            let offset_to = BlockPos::new(pos.x() + 2, pos.y() + y, pos.z() + 2);

            let layer = self.get_open_water_type_for_area(offset_from, offset_to);

            match layer {
                OpenWaterType::AboveWater => {
                    if prev_layer == OpenWaterType::Invalid {
                        return false;
                    }
                }
                OpenWaterType::InsideWater => {
                    if prev_layer == OpenWaterType::AboveWater {
                        return false;
                    }
                }

                OpenWaterType::Invalid => {
                    return false;
                }
            }
            prev_layer = layer;
        }

        true
    }

    /// Returns an `OpenWaterType` for a given area.
    fn get_open_water_type_for_area(&self, from: BlockPos, to: BlockPos) -> OpenWaterType {
        let mut iter =
            BlockPos::between_closed(from, to).map(|pos| self.get_open_water_type_for_block(pos));

        let Some(first) = iter.next() else {
            return OpenWaterType::Invalid;
        };

        if iter.all(|value| value == first) {
            first
        } else {
            OpenWaterType::Invalid
        }
    }

    /// Returns an `OpenWaterType` for a given block.
    fn get_open_water_type_for_block(&self, pos: BlockPos) -> OpenWaterType {
        let Some(world) = self.level() else {
            return OpenWaterType::Invalid;
        };

        let block_state = world.get_block_state(pos);
        let collision_shape = block_state.get_collision_shape_at(pos);

        if !block_state.is_air() && !(block_state.get_block() == &vanilla_blocks::LILY_PAD) {
            let fluid_state = block_state.get_fluid_state();
            if fluid_state.is_water() && fluid_state.is_source() && collision_shape.is_empty() {
                OpenWaterType::InsideWater
            } else {
                OpenWaterType::Invalid
            }
        } else {
            OpenWaterType::AboveWater
        }
    }

    /// `FishingHook.retrieve`: reels the hook in, pulling a hooked entity or
    /// landing a catch, and returns the damage the rod takes.
    pub fn retrieve(&self, rod: &ItemStack) -> i32 {
        let Some(owner) = self.player_owner() else {
            return 0;
        };
        let Some(player) = owner.as_player() else {
            return 0;
        };
        if self.should_stop_fishing(player) {
            return 0;
        }

        let mut damage = 0;
        let hooked = self.hook_state.lock().hooked_entity.clone();
        if let Some(hooked) = hooked {
            self.pull_entity(&hooked);
            // TODO: trigger FISHING_ROD_HOOKED once advancement criteria exist.
            self.broadcast_entity_event(EntityStatus::FishingRodReelIn);
            damage = if hooked.is::<ItemEntity>() {
                DMG_ITEM_ENTITY
            } else {
                DMG_DEFAULT
            };
        } else {
            let luck = {
                let state = self.hook_state.lock();
                (state.nibble > 0).then_some(state.luck)
            };
            if let Some(luck) = luck
                && let Some(world) = self.level()
            {
                let position = self.position();
                let mut rng = rng();
                let mut loot_ctx = LootContext::new(&mut rng)
                    .with_origin(position.x, position.y, position.z)
                    .with_tool(rod)
                    .with_this_entity(entity_loot_ref(self, None))
                    .with_luck(luck as f32 + player.living().luck());
                let items = vanilla_loot_tables::GAMEPLAY_FISHING.get_random_items(&mut loot_ctx);
                // TODO: trigger FISHING_ROD_HOOKED once advancement criteria exist.
                self.spawn_catch(items, &world, player);

                let orb = ExperienceOrbEntity::new(
                    &vanilla_entities::EXPERIENCE_ORB,
                    next_entity_id(),
                    player.position() + DVec3::new(0.0, 0.5, 0.5),
                    Arc::downgrade(&world),
                );
                orb.set_value(rand::random_range(1..=6));
                if let Err(error) = world.try_add_entity(Arc::new(orb)) {
                    log::error!("Failed to spawn experience orb: {error}");
                }
                damage = DMG_CAUGHT;
            }
        }

        if self.on_ground() {
            damage = DMG_ON_GROUND;
        }
        self.set_removed(RemovalReason::Discarded);
        damage
    }

    /// `FishingHook.pullEntity`: pulls `entity` a tenth of the way to the owner.
    fn pull_entity(&self, entity: &SharedEntity) {
        if let Some(owner) = self.projectile().owner() {
            let delta = (owner.position() - self.position()) * 0.1;
            entity.set_velocity(entity.velocity() + delta);
        }
    }

    /// Throws the caught items toward `player` and counts caught fish.
    fn spawn_catch(&self, items: Vec<ItemStack>, world: &Arc<World>, player: &Player) {
        const SPEED: f64 = 0.1;
        const ARC: f64 = 0.08;
        for item_stack in items {
            let delta = player.position() - self.position();
            let velocity = DVec3::new(
                delta.x * SPEED,
                delta.y * SPEED + delta.length().sqrt() * ARC,
                delta.z * SPEED,
            );
            let is_fish = item_stack.item().has_tag(&ItemTag::FISHES);
            world.spawn_item_with_velocity(self.position(), item_stack, velocity);
            if is_fish {
                player.award_custom_stat(&vanilla_custom_stats::FISH_CAUGHT);
            }
        }
    }

    /// Whether a flying hook has landed and should stop.
    fn should_stop(&self) -> bool {
        self.hook_state.lock().bobber_state == BobberState::Flying
            && (self.on_ground() || self.horizontal_collision())
    }

    /// Runs the bobber state machine; returns `false` when the tick ends here.
    fn tick_bobber(
        &self,
        entity: &SharedEntity,
        world: &World,
        is_in_water: bool,
        pos: BlockPos,
        liquid_height: f32,
    ) -> bool {
        let bobber_state = self.hook_state.lock().bobber_state;
        match bobber_state {
            BobberState::Flying => {
                {
                    let mut state = self.hook_state.lock();
                    if state.hooked_entity.is_some() {
                        self.set_velocity(DVec3::ZERO);
                        state.bobber_state = BobberState::HookedInEntity;
                        return false;
                    }
                    if is_in_water {
                        self.set_velocity(self.velocity() * DVec3::new(0.3, 0.2, 0.3));
                        state.bobber_state = BobberState::Bobbing;
                        return false;
                    }
                }
                if !self.on_ground() {
                    let projectile = self.projectile();
                    if let Some(hit) = projectile.hit_result_on_move_vector() {
                        projectile.hit_target_or_deflect_self(entity, &hit);
                    }
                }
                true
            }
            BobberState::HookedInEntity => {
                let hooked = self.hook_state.lock().hooked_entity.clone();
                let Some(hooked) = hooked else {
                    self.hook_state.lock().bobber_state = BobberState::Flying;
                    return false;
                };
                let same_dimension = hooked.level().is_some_and(|hooked_world| {
                    world.dimension_type == hooked_world.dimension_type
                });
                if !hooked.is_removed() && hooked.can_interact_with_level() && same_dimension {
                    let position = hooked.position()
                        + DVec3::new(0.0, hooked.bounding_box().height() * 0.8, 0.0);
                    if let Err(error) = self.try_set_position(position) {
                        self.set_removed(RemovalReason::Discarded);
                        log::error!("Failed to set position of fishing hook: {error}");
                    }
                } else {
                    self.set_hooked_entity(None);
                    self.hook_state.lock().bobber_state = BobberState::Flying;
                }
                false
            }
            BobberState::Bobbing => {
                let mut state = self.hook_state.lock();
                let velocity = self.velocity();
                let mut force =
                    self.position().y + velocity.y - f64::from(pos.y()) - f64::from(liquid_height);
                if force.abs() < 0.01 {
                    force += force.signum() * 0.1;
                }
                self.set_velocity(DVec3::new(
                    velocity.x * 0.9,
                    velocity.y - force * rng().random_range(0.0..0.2),
                    velocity.z * 0.9,
                ));

                if state.nibble <= 0 && state.time_until_hooked <= 0 {
                    state.open_water = true;
                } else {
                    state.open_water = state.open_water
                        && state.out_of_water_time < MAX_OUT_OF_WATER_TIME
                        && self.calculate_open_water(pos);
                }

                if is_in_water {
                    state.out_of_water_time = (state.out_of_water_time - 1).max(0);
                    if *self.entity_data.lock().fishing_hook.biting.get() {
                        let mut random = self.synchronized_random.lock();
                        let pull = -0.1 * random.next_f32() * random.next_f32();
                        self.set_velocity(self.velocity() + DVec3::new(0.0, f64::from(pull), 0.0));
                    }
                    self.catching_fish(pos, &mut state);
                } else {
                    state.out_of_water_time =
                        (state.out_of_water_time + 1).min(MAX_OUT_OF_WATER_TIME);
                }
                true
            }
        }
    }

    /// A hook lying on the ground for a minute disappears.
    fn tick_life(&self) {
        if !self.on_ground() {
            self.hook_state.lock().life = 0;
            return;
        }
        let expired = {
            let mut state = self.hook_state.lock();
            state.life += 1;
            state.life >= ONE_MINUTE
        };
        if expired {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    /// `FishingHook.tick`.
    fn entity_tick(&self, entity: &SharedEntity) {
        if let Some(world) = self.level() {
            let seed = self.uuid().as_u64_pair().1 as i64 ^ world.game_time();
            self.synchronized_random.lock().set_seed(seed);
        }
        self.projectile().tick();

        let Some(owner) = self.player_owner() else {
            self.set_removed(RemovalReason::Discarded);
            return;
        };
        let Some(player) = owner.as_player() else {
            return;
        };
        if self.should_stop_fishing(player) {
            return;
        }
        self.tick_life();

        let Some(world) = self.level() else {
            return;
        };
        let pos = BlockPos::from(self.position());
        let fluid_state = world.get_block_state(pos).get_fluid_state();
        let liquid_height = if fluid_state.is_water() {
            get_height(&world, pos, fluid_state)
        } else {
            0.0
        };
        if !self.tick_bobber(entity, &world, liquid_height > 0.0, pos, liquid_height) {
            return;
        }

        if !fluid_state.is_water()
            && !self.on_ground()
            && self.hook_state.lock().hooked_entity.is_none()
        {
            self.set_velocity(self.velocity() + DVec3::new(0.0, -0.03, 0.0));
        }
        Arc::clone(entity).move_entity(MoverType::SelfMovement, self.velocity());
        self.apply_effects_from_blocks();
        self.projectile().update_rotation();
        if self.should_stop() {
            self.set_velocity(DVec3::ZERO);
        }
        self.set_velocity(self.velocity() * 0.92);
        self.set_old_position_to_current();
    }
}

/// Collection of possible states the fishing bobber of the `FishingHookEntity` can take on.
/// Equivalent to Java's `FishingHook.FishHookState` (we renamed for clarity)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BobberState {
    Flying,
    HookedInEntity,
    Bobbing,
}

/// Collection of possible types associated with open water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenWaterType {
    AboveWater,
    InsideWater,
    Invalid,
}

#[cfg(test)]
mod tests;
