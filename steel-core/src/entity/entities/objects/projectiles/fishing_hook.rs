use crate::entity::entities::{ExperienceOrbEntity, ItemEntity};
use crate::entity::projectile::triangle_random;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, Projectile, ProjectileBase,
    RemovalReason, SharedEntity, ThrowableProjectile, entity_loot_ref, next_entity_id,
};
use crate::fluid::get_height;
use crate::physics::MoverType;
use crate::player::Player;
use crate::world::{LevelReader, World};
use glam::DVec3;
use rand::{RngExt, rng};
use std::cmp::PartialEq;
use std::f32::consts::PI;
use std::ops::Add;
use std::sync::{Arc, Weak};
use steel_macros::entity_behavior;
use steel_math::trig;
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
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Downcast, DowncastType, DowncastTypeKey};

pub const MAX_OUT_OF_WATER_TIME: i32 = 10;
const MAX_DISTANCE_SQR: f64 = 32.0 * 32.0;

const DMG_DEFAULT: i32 = 5;
const DMG_ITEM_ENTITY: i32 = 3;
const DMG_ON_GROUND: i32 = 2;
const DMG_CAUGHT: i32 = 1;

const DEGREE_180: f32 = 180.0;
const DEGREE_360: f32 = 360.0;
const DEG_TO_RAD: f32 = PI / DEGREE_180;

const ONE_SECOND: i32 = 20;
const TWO_SECONDS: i32 = 40;
const THREE_SECONDS: i32 = 60;
const FOUR_SECONDS: i32 = 80;
const FIVE_SECONDS: i32 = 100;
const THIRTY_SECONDS: i32 = 600;
const ONE_MINUTE: i32 = 1200;

/// A fishing hook.
#[entity_behavior(class = "FishingHook")]
pub struct FishingHookEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<FishingBobberEntityData>,
    projectile_base: ProjectileBase,
    hook_state: SyncMutex<FishingHookState>,
    synchronized_random: SyncMutex<LegacyRandom>,
}

/// This struct holds entity specific state information per fishing hook entity.
pub struct FishingHookState {
    out_of_water_time: i32,
    life: i32,
    nibble: i32,
    time_until_lured: i32,
    time_until_hooked: i32,
    fish_angle: f32,
    open_water: bool,
    /// Equivalent to Java's `currentState`
    bobber_state: BobberState,
    hooked_entity: Option<SharedEntity>,
    luck: i32,
    lure_speed: i32,
}

impl FishingHookState {
    #[must_use]
    /// Returns a new `FishingHookState` with the given lure speed and luck values.
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

// SAFETY: This key is owned by Steel and uniquely identifies `FishingHookEntity`.
unsafe impl DowncastType for FishingHookEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/fishing_hook");
}

impl FishingHookEntity {
    /// Creates a fishing hook entity.
    /// We keep both this generic constructor and `shoot_from_player` in order to ensure future-proofing in terms of a future plugin API.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(FishingBobberEntityData::new()),
            projectile_base: ProjectileBase::new(),
            hook_state: SyncMutex::new(FishingHookState::new(0, 0)),
            synchronized_random: SyncMutex::new(LegacyRandom::from_seed(0)),
        }
    }

    /// Mimics Java's `FishingHook(Player, Level, int, int)` constructor. (But we don't need `level` here)
    pub fn shoot_from_player(self: &Arc<Self>, player: &Arc<Player>, luck: i32, lure_speed: i32) {
        const MAGIC_OFFSET: f64 = 0.010_336_5;

        {
            let mut state = self.hook_state.lock();
            state.luck = luck.max(0);
            state.lure_speed = lure_speed.max(0);
        }

        let (yaw, pitch) = player.rotation();
        let player_shared: SharedEntity = player.clone();

        self.set_owner(&player_shared);

        let y_cos = trig::cos(f64::from(-yaw * DEG_TO_RAD - PI));
        let y_sin = trig::sin(f64::from(-yaw * DEG_TO_RAD - PI));
        let x_cos = -trig::cos(f64::from(-pitch * DEG_TO_RAD));
        let x_sin = trig::sin(f64::from(-pitch * DEG_TO_RAD));

        let x = player_shared.position().x - f64::from(y_sin) * 0.3;
        let y = player_shared.get_eye_y();
        let z = player_shared.position().z - f64::from(y_cos) * 0.3;

        self.snap_to(DVec3::new(x, y, z), yaw, pitch);

        let clamped_y = f64::from((-(x_sin / x_cos)).clamp(-5.0, 5.0));

        let mut new_movement = DVec3::new(-f64::from(y_sin), clamped_y, -f64::from(y_cos));

        let distance = new_movement.length();

        let random_x = triangle_random(0.5, MAGIC_OFFSET);
        let random_y = triangle_random(0.5, MAGIC_OFFSET);
        let random_z = triangle_random(0.5, MAGIC_OFFSET);

        let factor_x = 0.6 / distance + random_x;
        let factor_y = 0.6 / distance + random_y;
        let factor_z = 0.6 / distance + random_z;

        new_movement *= DVec3::new(factor_x, factor_y, factor_z);

        self.set_velocity(new_movement);

        let yaw_new = new_movement.x.atan2(new_movement.z).to_degrees() as f32;

        let horizontal_distance =
            (new_movement.x * new_movement.x + new_movement.z * new_movement.z).sqrt();

        let pitch_new = new_movement.y.atan2(horizontal_distance).to_degrees() as f32;

        self.set_rotation((yaw_new, pitch_new));
        self.base().set_old_rotation_to_current();
    }

    /// Creates a fishing hook entity from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(FishingBobberEntityData::new()),
            projectile_base: ProjectileBase::new(),
            // FIXME: `lure_speed` and `luck` are taken from the existing rod, but auto generation fails when doing this, refer to: https://mcsrc.dev/2/26.2/net/minecraft/world/entity/projectile/FishingHook#L75
            hook_state: SyncMutex::new(FishingHookState::new(0, 0)),
            synchronized_random: SyncMutex::new(LegacyRandom::from_seed(0)),
        }
    }

    /// Sets the projectile owner and mirrors vanilla's `Player.fishing` update.
    pub(crate) fn set_owner(self: &Arc<Self>, owner: &SharedEntity) {
        self.set_owner_entity(Some(owner));
        if let Some(player) = owner.as_player() {
            player.set_fishing_hook(self);
        }
        self.update_owner_info(self.into());
    }

    /// Determines if the player should stop fishing and removes the entity if so.
    fn should_stop_fishing(
        &self,
        owner: &Player,
        mainhand_item: &ItemStack,
        offhand_item: &ItemStack,
    ) -> bool {
        if !owner.can_interact_with_level() {
            self.set_removed(RemovalReason::Discarded);
            return true;
        }

        let mainhand_fishing = mainhand_item.is(&vanilla_items::FISHING_ROD);
        let offhand_fishing = offhand_item.is(&vanilla_items::FISHING_ROD);

        if (mainhand_fishing || offhand_fishing)
            && self.distance_to_sqr(owner.position()) <= MAX_DISTANCE_SQR
        {
            return false;
        }

        self.set_removed(RemovalReason::Discarded);
        true
    }

    /// Determines if the fishing hook should hit a target or be deflected.
    fn check_collision(&self) {
        if let Some(hit_result) = self.get_hit_result_on_move_vector() {
            self.hit_target_or_deflect_self(&hit_result);
        }
    }

    /// Stores the currently hooked entity inside `entity_data`.
    fn set_hooked_entity(&self, hooked: Option<SharedEntity>) {
        let hooked_entity_id = hooked.as_ref().map_or(0, |entity| {
            let id = entity.base().id();
            id + 1
        });

        {
            let mut hook_state = self.hook_state.lock();
            hook_state.hooked_entity = hooked;
        }

        let mut entity_data = self.entity_data.lock();

        entity_data.fishing_hook.hooked_entity.set(hooked_entity_id);
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
                self.entity_data.lock().fishing_hook_mut().biting.set(false);
            }
        } else if state.time_until_hooked > 0 {
            state.time_until_hooked -= fishing_speed;

            if state.time_until_hooked > 0 {
                state.fish_angle += triangle_random(0.0, 9.188) as f32;

                let angle = state.fish_angle * DEG_TO_RAD;
                let angle_sin = trig::sin(f64::from(angle));
                let angle_cos = trig::cos(f64::from(angle));

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
                // I check for the world here first, because we don't need to invoke `y` (the only call using `world` is the one using `y` through `particle_pos`) if it doesn't exist
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
                self.entity_data.lock().fishing_hook_mut().biting.set(true);
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
                // same reason to call this early in here as well: no need to calculate the rest if there is no world to spawn the particle in.
                let Some(world) = self.level() else {
                    return;
                };

                let angle = rng().random_range(0.0..=DEGREE_360) * DEG_TO_RAD;
                let dist = rng().random_range(25.0..=60.0);

                let fish_x =
                    self.position().x + f64::from(trig::sin(f64::from(angle))) * dist * 0.1;
                let fish_y = self.position().y.floor() + 1.0;
                let fish_z =
                    self.position().z + f64::from(trig::cos(f64::from(angle))) * dist * 0.1;

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

    /// Retrieves the entity caught by this fishing hook and returns the resulting damage value.
    pub fn retrieve(&self, rod: &ItemStack) -> i32 {
        let mut damage = 0;

        if let Some(owner) = self.get_owner()
            && let Some(player) = owner.as_player()
        {
            let can_retrieve = {
                let inventory = player.inventory.lock();
                let mainhand_item = inventory.get_item_in_hand(InteractionHand::MainHand);
                let offhand_item = inventory.get_offhand_item();

                !Self::should_stop_fishing(self, player, mainhand_item, offhand_item)
            };

            if can_retrieve {
                let hooked_in = {
                    let hook_state = self.hook_state.lock();
                    hook_state.hooked_entity.clone()
                };

                if let Some(hooked_in) = hooked_in {
                    self.pull_entity(&hooked_in);
                    // TODO: criteria triggers (advancements)
                    self.broadcast_entity_event(EntityStatus::FishingRodReelIn);
                    damage = if hooked_in.as_ref().is::<ItemEntity>() {
                        DMG_ITEM_ENTITY
                    } else {
                        DMG_DEFAULT
                    };
                } else {
                    let luck = {
                        let state = self.hook_state.lock();

                        (state.nibble > 0).then_some(state.luck)
                    };

                    if let Some(luck) = luck {
                        let mut rng = rng();

                        // This is equivalent to `LootParams params` in the java src.
                        let mut loot_ctx = LootContext::new(&mut rng)
                            .with_origin(self.position().x, self.position().y, self.position().z)
                            .with_tool(rod)
                            .with_this_entity(entity_loot_ref(self))
                            .with_luck(luck as f32 + player.get_luck());

                        let items =
                            vanilla_loot_tables::GAMEPLAY_FISHING.get_random_items(&mut loot_ctx);

                        // TODO: criteria triggers (advancements)

                        let Some(world) = self.level() else {
                            return damage;
                        };

                        self.spawn_loot_award_stat(items, world.clone(), owner.clone());

                        let orb_pos = DVec3::new(
                            player.position().x,
                            player.position().y + 0.5,
                            player.position().z + 0.5,
                        );

                        let orb = ExperienceOrbEntity::new(
                            &vanilla_entities::EXPERIENCE_ORB,
                            next_entity_id(),
                            orb_pos,
                            Arc::downgrade(&world),
                        );

                        orb.set_value(rand::random_range(1..=6));

                        let entity: SharedEntity = Arc::new(orb);

                        if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
                            log::error!("Failed to spawn experience orb: {error}");
                        }

                        damage = DMG_CAUGHT;
                    }
                }

                if self.base.on_ground() {
                    damage = DMG_ON_GROUND;
                }

                self.set_removed(RemovalReason::Discarded);
            }
        }
        damage
    }

    /// Modifies the hooked entities velocity in order to simulate a pulling motion.
    fn pull_entity(&self, entity: &Arc<dyn Entity>) {
        if let Some(owner) = self.get_owner() {
            let base = owner.base();
            let delta = DVec3::new(
                base.position().x - self.base.position().x,
                base.position().y - self.base.position().y,
                base.position().z - self.base.position().z,
            ) * 0.1;
            entity.set_velocity(entity.velocity().add(delta));
        }
    }

    /// Clears owner info of this `FishingHookEntity`
    fn clear_owner_info(&self) {
        let Some(owner) = self.get_owner() else {
            return;
        };
        let Some(player) = owner.as_player() else {
            return;
        };

        player.clear_fishing_hook(self);
    }

    /// Clears owner info if `hook` is `None` and stores it, if it is `Some`
    fn update_owner_info(&self, hook: Option<&Arc<FishingHookEntity>>) {
        if let Some(owner) = self.get_owner()
            && let Some(player) = owner.as_player()
        {
            match hook {
                Some(hook) => player.set_fishing_hook(hook),
                None => player.clear_fishing_hook(self),
            }
        }
    }

    // I added this fn because I thought it would be cleaner this way, it's not in the vanilla src, but how I use it ensures vanilla behavior
    /// Loops through a `vec` of `ItemStack`s (the fishing loot), spawns them as `ItemEntity`s in the world and awards the stat `FISH_CAUGHT`
    fn spawn_loot_award_stat(
        &self,
        items: Vec<ItemStack>,
        world: Arc<World>,
        owner: Arc<dyn Entity>,
    ) {
        for item_stack in items {
            const SPEED: f64 = 0.1;
            const INVERSE_CUBE: f64 = 0.08;

            if let Some(player) = owner.as_player() {
                let xa = player.position().x - self.position().x;
                let ya = player.position().y - self.position().y;
                let za = player.position().z - self.position().z;

                let vel = DVec3::new(
                    xa * SPEED,
                    ya * SPEED + (xa * xa + ya * ya + za * za).sqrt().sqrt() * INVERSE_CUBE,
                    za * SPEED,
                );

                World::spawn_item_with_velocity(&world, self.position(), item_stack.clone(), vel);

                if item_stack.item().has_tag(&ItemTag::FISHES) {
                    player.award_custom_stat(&vanilla_custom_stats::FISH_CAUGHT);
                }
            } else {
                return;
            }
        }
    }

    // TODO: check if passing a lock is better here
    /// Determines if the player should stop fishing.
    fn should_stop(&self) -> bool {
        let state = self.hook_state.lock();

        state.bobber_state == BobberState::Flying
            && (self.base.on_ground() || self.base.horizontal_collision())
    }

    /// Bobber specific ticking logic. We return a `bool` here, so we can return early inside `tick`.
    fn tick_bobber(
        &self,
        bobber_state: BobberState,
        world: &World,
        is_in_water: bool,
        pos: BlockPos,
        liquid_height: f32,
    ) -> bool {
        match bobber_state {
            BobberState::Flying => {
                let should_check_collision = {
                    let mut state = self.hook_state.lock();

                    if state.hooked_entity.is_some() {
                        self.base.set_velocity(DVec3::ZERO);
                        state.bobber_state = BobberState::HookedInEntity;
                        return false;
                    }

                    if is_in_water {
                        self.base
                            .set_velocity(self.base.velocity() * DVec3::new(0.3, 0.2, 0.3));
                        state.bobber_state = BobberState::Bobbing;
                        return false;
                    }

                    !self.on_ground()
                };

                if should_check_collision {
                    self.check_collision();
                }

                true
            }

            BobberState::HookedInEntity => {
                let hooked = {
                    let state = self.hook_state.lock();
                    state.hooked_entity.clone()
                };

                let Some(hooked) = hooked else {
                    let mut state = self.hook_state.lock();
                    state.bobber_state = BobberState::Flying;
                    return false;
                };

                let removed = hooked.is_removed();
                let can_interact = hooked.can_interact_with_level();

                // locks hooked.base.world
                let same_dimension = if let Some(hooked_world) = hooked.level() {
                    world.dimension_type == hooked_world.dimension_type
                } else {
                    false
                };

                if !removed && can_interact && same_dimension {
                    let pos = hooked.position();
                    let height = hooked.bounding_box().height();

                    if let Err(error) =
                        self.try_set_position(DVec3::new(pos.x, pos.y + height * 0.8, pos.z))
                    {
                        self.set_removed(RemovalReason::Discarded);
                        log::error!("Failed to set position of fishing hook: {error}");
                    }
                } else {
                    self.set_hooked_entity(None);

                    let mut state = self.hook_state.lock();
                    state.bobber_state = BobberState::Flying;
                }

                false
            }

            BobberState::Bobbing => {
                let mut state = self.hook_state.lock();

                let velocity = self.base.velocity();

                let mut force: f64 =
                    self.position().y + velocity.y - f64::from(pos.y()) - f64::from(liquid_height);

                if force.abs() < 0.01 {
                    force += force.signum() * 0.1;
                }

                self.base.set_velocity(DVec3::new(
                    velocity.x * 0.9,
                    velocity.y - force * rng().random::<f64>() * 0.2,
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
                    if *self.entity_data.lock().fishing_hook().biting.get() {
                        let mut synchronized_random = self.synchronized_random.lock();
                        self.base.set_velocity(self.base.velocity().add(DVec3::new(
                            0.0,
                            f64::from(
                                -0.1 * synchronized_random.next_f32()
                                    * synchronized_random.next_f32(),
                            ),
                            0.0,
                        )));
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

    fn tick_life(&self) {
        if self.on_ground() {
            let should_remove = {
                let mut state = self.hook_state.lock();
                state.life += 1;
                state.life >= ONE_MINUTE
            };

            if should_remove {
                self.set_removed(RemovalReason::Discarded);
            }
        } else {
            self.hook_state.lock().life = 0;
        }
    }

    fn is_hooked_in(&self) -> bool {
        let state = self.hook_state.lock();
        state.hooked_entity.is_some()
    }

    fn bobber_state(&self) -> BobberState {
        let state = self.hook_state.lock();
        state.bobber_state
    }

    fn can_fish(&self, player: &Player) -> bool {
        let inventory = player.inventory.lock();
        let mainhand_item = inventory.get_item_in_hand(InteractionHand::MainHand);
        let offhand_item = inventory.get_offhand_item();

        !self.should_stop_fishing(player, mainhand_item, offhand_item)
    }
}

impl Entity for FishingHookEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    /// Responsible for all state-changes.
    fn tick(&self) {
        {
            let mut synchronized_random = self.synchronized_random.lock();
            let least_significant_bits = self.uuid().as_u64_pair().1;

            if let Some(world) = self.level() {
                let game_time = world.game_time();
                let seed = least_significant_bits as i64 ^ game_time;

                synchronized_random.set_seed(seed);
            }
        }

        self.projectile_base_tick();
        if let Some(owner) = self.get_owner()
            && let Some(player) = owner.as_player()
        {
            if self.can_fish(player) {
                self.tick_life();

                let pos = BlockPos::from(self.base.position());

                if let Some(world) = self.level() {
                    let block_state = world.get_block_state(pos);
                    let fluid_state = block_state.get_fluid_state();

                    let liquid_height = {
                        if fluid_state.is_water() {
                            get_height(&world, pos, fluid_state)
                        } else {
                            0.0
                        }
                    };

                    let is_in_water = liquid_height > 0.0;

                    if !self.tick_bobber(
                        self.bobber_state(),
                        &world,
                        is_in_water,
                        pos,
                        liquid_height,
                    ) {
                        return;
                    }

                    if !fluid_state.is_water() && !self.base.on_ground() && !self.is_hooked_in() {
                        self.base
                            .set_velocity(self.base.velocity().add(DVec3::new(0.0, -0.03, 0.0)));
                    }

                    self.move_entity(MoverType::SelfMovement, self.base.velocity());
                    self.apply_effects_from_blocks();
                    self.update_rotation();

                    // TODO: check if passing a lock is better here
                    if self.should_stop() {
                        self.base.set_velocity(DVec3::ZERO);
                    }

                    let inertia: f64 = 0.92;
                    self.base.set_velocity(self.base.velocity() * inertia);
                    self.base.set_old_position_to_current();
                }
            } else {
                self.set_removed(RemovalReason::Discarded);
            }
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    /// Marks entity as removed and clears owner info.
    fn set_removed(&self, reason: RemovalReason) {
        self.clear_owner_info();
        self.base.set_removed(reason);
    }

    /// Returns the ID of the owner, or of this `FishingHookEntity`, if it has no owner.
    fn spawn_data(&self) -> i32 {
        self.get_owner().map_or(self.id(), |owner| owner.id())
    }
}

impl Projectile for FishingHookEntity {
    /// Returns this `FishingHook`s `ProjectileBase`.
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    /// Determines if it's possible to hit an entity.
    fn can_hit_entity(&self, entity: &dyn Entity) -> bool {
        self.base_can_hit_entity(entity) || (entity.is_alive() && entity.is::<ItemEntity>())
    }

    /// Stores the hit entity inside `hooked_in`.
    fn on_hit_entity(&self, entity: &SharedEntity, _location: DVec3) {
        self.set_hooked_entity(Some(Arc::clone(entity)));
    }
}

impl ThrowableProjectile for FishingHookEntity {}

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
mod tests {
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_entities;
    use uuid::Uuid;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    fn test_hook(world: &Arc<World>, id: i32) -> Arc<FishingHookEntity> {
        Arc::new(FishingHookEntity::new(
            &vanilla_entities::FISHING_BOBBER,
            id,
            DVec3::ZERO,
            Arc::downgrade(world),
        ))
    }

    #[test]
    fn spawn_data_identifies_the_owning_player() {
        let world = fresh_test_world("fishing_hook_spawn_data");
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(1), 37).build();
        let owner: SharedEntity = player;
        let hook = test_hook(&world, 38);
        hook.set_owner_entity(Some(&owner));

        assert_eq!(hook.spawn_data(), owner.id());
    }

    #[test]
    fn removal_only_clears_the_matching_active_hook() {
        let world = fresh_test_world("fishing_hook_owner_lifecycle");
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(2), 40).build();
        let player_owner = Arc::clone(&player);
        let owner: SharedEntity = player_owner;
        let first = test_hook(&world, 41);
        let second = test_hook(&world, 42);

        first.set_owner(&owner);
        assert!(
            player
                .fishing_hook()
                .is_some_and(|active| Arc::ptr_eq(&active, &first))
        );

        second.set_owner(&owner);
        first.set_removed(RemovalReason::Discarded);
        assert!(
            player
                .fishing_hook()
                .is_some_and(|active| Arc::ptr_eq(&active, &second))
        );

        second.set_removed(RemovalReason::Discarded);
        assert!(player.fishing_hook().is_none());
    }

    #[test]
    fn retrieving_discards_the_active_hook() {
        let world = fresh_test_world("fishing_hook_retrieve_lifecycle");
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(3), 50).build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
        let player_owner = Arc::clone(&player);
        let owner: SharedEntity = player_owner;
        let hook = test_hook(&world, 51);
        hook.set_owner(&owner);
        let rod = ItemStack::new(&vanilla_items::FISHING_ROD);

        assert_eq!(hook.retrieve(&rod), 0);
        assert!(hook.is_removed());
        assert!(player.fishing_hook().is_none());
    }

    #[test]
    fn shoot_from_player_respects_pitch_and_yaw_signs() {
        let world = fresh_test_world("fishing_hook_shoot_signs");

        // Straight down: pitch = 90.0, yaw = 0.0 -> Y velocity must be negative (downwards)
        let player_down =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(10), 100).build();
        player_down.set_rotation((0.0, 90.0));
        let hook_down = test_hook(&world, 101);
        hook_down.shoot_from_player(&player_down, 0, 0);
        assert!(
            hook_down.velocity().y < -2.0,
            "Looking straight down must throw downwards, got y={}",
            hook_down.velocity().y
        );

        // Straight up: pitch = -90.0, yaw = 0.0 -> Y velocity must be positive (upwards)
        let player_up =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(11), 110).build();
        player_up.set_rotation((0.0, -90.0));
        let hook_up = test_hook(&world, 111);
        hook_up.shoot_from_player(&player_up, 0, 0);
        assert!(
            hook_up.velocity().y > 2.0,
            "Looking straight up must throw upwards, got y={}",
            hook_up.velocity().y
        );

        // West: yaw = 90.0, pitch = 0.0 -> X velocity must be negative (-X is West)
        let player_west =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(12), 120).build();
        player_west.set_rotation((90.0, 0.0));
        let hook_west = test_hook(&world, 121);
        hook_west.shoot_from_player(&player_west, 0, 0);
        assert!(
            hook_west.velocity().x < -0.8,
            "Looking West must throw in -X direction, got x={}",
            hook_west.velocity().x
        );

        // East: yaw = -90.0, pitch = 0.0 -> X velocity must be positive (+X is East)
        let player_east =
            TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(13), 130).build();
        player_east.set_rotation((-90.0, 0.0));
        let hook_east = test_hook(&world, 131);
        hook_east.shoot_from_player(&player_east, 0, 0);
        assert!(
            hook_east.velocity().x > 0.8,
            "Looking East must throw in +X direction, got x={}",
            hook_east.velocity().x
        );
    }

    #[test]
    fn grounded_hook_does_not_hook_owner_when_player_stands_on_it() {
        steel_registry::init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("fishing_hook_grounded_owner");
        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(20), 200).build();
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
        let player_owner = Arc::clone(&player);
        let owner: SharedEntity = player_owner;
        let hook = test_hook(&world, 201);
        hook.set_owner(&owner);
        hook.set_on_ground(true);
        hook.set_velocity(DVec3::ZERO);

        // Position hook inside player's bounding box
        hook.try_set_position(player.position())
            .expect("should position hook");

        hook.tick();

        let hooked_entity = hook.hook_state.lock().hooked_entity.clone();
        assert!(
            hooked_entity.is_none(),
            "Grounded stationary hook must not hook player standing on it"
        );
    }

    #[test]
    fn submerged_hook_in_water_experiences_upward_buoyancy() {
        use crate::test_support::insert_ready_full_chunk;
        use steel_utils::ChunkPos;
        use steel_utils::types::UpdateFlags;

        steel_registry::init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("fishing_hook_buoyancy");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let water = vanilla_blocks::WATER.default_state();
        let pos_submerged = BlockPos::new(0, 60, 0);
        let pos_above = BlockPos::new(0, 61, 0);
        world.set_block(pos_submerged, water, UpdateFlags::UPDATE_NONE);
        world.set_block(pos_above, water, UpdateFlags::UPDATE_NONE);

        let player = TestPlayerBuilder::new(Arc::clone(&world), Uuid::from_u128(30), 300).build();
        player
            .try_set_position(DVec3::new(0.5, 61.0, 0.5))
            .expect("should position player near water");
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::FISHING_ROD));
        let player_owner = Arc::clone(&player);
        let owner: SharedEntity = player_owner;

        let hook = test_hook(&world, 301);
        hook.set_owner(&owner);
        hook.try_set_position(DVec3::new(0.5, 60.5, 0.5))
            .expect("should position hook");
        hook.set_velocity(DVec3::ZERO);
        hook.hook_state.lock().bobber_state = BobberState::Bobbing;

        hook.tick();

        assert!(
            hook.velocity().y > 0.0,
            "Submerged hook must accelerate upwards towards the water surface, got velocity.y={}",
            hook.velocity().y
        );
    }

    #[test]
    fn open_water_calculation_identifies_open_lake_and_shallow_puddle() {
        use crate::test_support::insert_ready_full_chunk;
        use steel_utils::ChunkPos;
        use steel_utils::types::UpdateFlags;

        steel_registry::init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("fishing_hook_open_water");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let water = vanilla_blocks::WATER.default_state();
        let air = vanilla_blocks::AIR.default_state();
        let center = BlockPos::new(8, 64, 8);

        // Build 5x5 open water area:
        // y in -1..=0: water
        // y in 1..=2: air
        for y in -1..=0 {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    world.set_block(
                        BlockPos::new(center.x() + dx, center.y() + y, center.z() + dz),
                        water,
                        UpdateFlags::UPDATE_NONE,
                    );
                }
            }
        }
        for y in 1..=2 {
            for dx in -2..=2 {
                for dz in -2..=2 {
                    world.set_block(
                        BlockPos::new(center.x() + dx, center.y() + y, center.z() + dz),
                        air,
                        UpdateFlags::UPDATE_NONE,
                    );
                }
            }
        }

        let hook = test_hook(&world, 401);
        assert!(
            hook.calculate_open_water(center),
            "5x5 open water lake must be considered open water"
        );

        // Place a solid block in the water layer -> should no longer be open water
        world.set_block(
            BlockPos::new(center.x() + 1, center.y(), center.z()),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        );
        assert!(
            !hook.calculate_open_water(center),
            "Obstructed water area must not be considered open water"
        );
    }
}
