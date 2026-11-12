//! Experience orb entity implementation.

use crate::entity::component::definition::Definition;
use crate::entity::component::definition::HurtCtx;
use crate::entity::component::pipeline::Pipeline;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::prelude::*;
use std::sync::{Arc, Weak};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::nbt::NbtValueInput as _;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_math::{DEGREE_360, vector};
use steel_protocol::packets::game::{CTakeItemEntity, SoundSource};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::ExperienceOrbEntityData;
use steel_utils::locks::CheckedMutex;
use steel_utils::{ChunkPos, Downcast as _, WorldAabb};

use crate::entity::EntityProps;
use crate::entity::damage::DamageSource;
use crate::entity::{
    DefaultEntityPush, Entity, EntityBase, EntityMovementEmission, RemovalReason, SharedEntity,
    next_entity_id,
};
use crate::fluid::get_fluid_state;
use crate::physics::{MoverType, WorldCollisionProvider, has_collision};
use crate::player::Player;
use crate::world::World;

const LIFETIME: i32 = 6000;
const ENTITY_SCAN_PERIOD: i32 = 20;
const MAX_FOLLOW_DIST: f64 = 8.0;
const MAX_FOLLOW_DIST_SQR: f64 = MAX_FOLLOW_DIST * MAX_FOLLOW_DIST;
const ORB_GROUPS_PER_AREA: i32 = 40;
const ORB_MERGE_DISTANCE: f64 = 0.5;
const DEFAULT_HEALTH: i32 = 5;

struct ExperienceOrbState {
    age: i32,
    health: i32,
    count: i32,
    following_player_id: Option<i32>,
}

impl ExperienceOrbState {
    const fn new() -> Self {
        Self {
            age: 0,
            health: DEFAULT_HEALTH,
            count: 1,
            following_player_id: None,
        }
    }
}

/// Vanilla experience orb entity.
#[entity_behavior(class = "ExperienceOrb")]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_orb, load = Self::load_orb)]
#[entity(key = "steel:entity/experience_orb", constructors, definition = Self::define)]
pub struct ExperienceOrbEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(synced)]
    entity_data: CheckedMutex<ExperienceOrbEntityData>,
    state: CheckedMutex<ExperienceOrbState>,
}

impl ExperienceOrbEntity {
    /// Creates a new experience orb with a value and vanilla spawn motion.
    #[must_use]
    pub fn with_value(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        value: i32,
        world: Weak<World>,
    ) -> Arc<Self> {
        let entity = Self::new(entity_type, id, position, world);
        entity.set_value(value);
        entity.initialize_spawn_movement();
        entity.unstuck_if_possible();
        entity
    }

    /// The end of the vanilla constructor: an orb spawned inside a collision is
    /// moved to the nearest free position within its own size.
    fn unstuck_if_possible(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let provider = WorldCollisionProvider::for_entity(&world, self);
        if !has_collision(&WorldCollisionProvider::new(&world), self.bounding_box()) {
            return;
        }
        let size = self.bounding_box().size();
        let dimensions = self.base.dimensions();
        let half_height = f64::from(dimensions.height) / 2.0;
        let center = self.position() + DVec3::new(0.0, half_height, 0.0);
        let allowed_centers = [WorldAabb::of_size(center, size, size, size)];
        let width = f64::from(dimensions.width);
        if let Some(free_center) = provider.find_free_position(
            &allowed_centers,
            center,
            width,
            f64::from(dimensions.height),
            width,
        ) && let Err(error) =
            self.try_set_position(free_center - DVec3::new(0.0, half_height, 0.0))
        {
            log::debug!("failed to unstick experience orb {}: {error}", self.id());
        }
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        Self {
            base,
            entity_type,
            entity_data: CheckedMutex::new(ExperienceOrbEntityData::new()),
            state: CheckedMutex::new(ExperienceOrbState::new()),
        }
    }

    /// Spawns vanilla experience orbs for an XP amount.
    pub fn award(world: &Arc<World>, position: DVec3, mut amount: i32) {
        while amount > 0 {
            let value = Self::get_experience_value(amount);
            amount -= value;
            if Self::try_merge_to_existing(world, position, value) {
                continue;
            }

            let entity: SharedEntity = Self::with_value(
                &vanilla_entities::EXPERIENCE_ORB,
                next_entity_id(),
                position,
                value,
                Arc::downgrade(world),
            );
            if let Err(error) = world.try_add_entity(entity) {
                log::debug!("failed to add experience orb: {error}");
            }
        }
    }

    /// Splits a total XP amount into the largest single orb value at most
    /// `max_value`, from a fixed tier table capped at 2477.
    #[must_use]
    pub const fn get_experience_value(max_value: i32) -> i32 {
        if max_value >= 2477 {
            2477
        } else if max_value >= 1237 {
            1237
        } else if max_value >= 617 {
            617
        } else if max_value >= 307 {
            307
        } else if max_value >= 149 {
            149
        } else if max_value >= 73 {
            73
        } else if max_value >= 37 {
            37
        } else if max_value >= 17 {
            17
        } else if max_value >= 7 {
            7
        } else if max_value >= 3 {
            3
        } else {
            1
        }
    }

    /// Returns this orb's XP value.
    #[must_use]
    pub fn value(&self) -> i32 {
        *self.entity_data.lock().value.get()
    }

    /// Sets this orb's XP value.
    pub fn set_value(&self, value: i32) {
        self.entity_data.lock().value.set(value);
    }

    /// Returns this orb's merge count.
    #[must_use]
    pub fn count(&self) -> i32 {
        self.state.lock().count
    }

    /// Returns this orb's age.
    #[must_use]
    pub fn age(&self) -> i32 {
        self.state.lock().age
    }

    /// Sets this orb's age.
    pub fn set_age(&self, age: i32) {
        self.state.lock().age = age;
    }

    /// Returns this orb's health.
    #[must_use]
    pub fn health(&self) -> i32 {
        self.state.lock().health
    }

    fn initialize_spawn_movement(&self) {
        let yaw = rand::random_range(0.0..DEGREE_360);
        let velocity = DVec3::new(
            f64::from(rand::random_range(-0.1f32..0.1)) * 2.0,
            f64::from(rand::random_range(0.0f32..0.2)) * 2.0,
            f64::from(rand::random_range(-0.1f32..0.1)) * 2.0,
        );
        self.base.set_rotation((yaw, 0.0));
        self.base.set_velocity(velocity);
    }

    fn try_merge_to_existing(world: &Arc<World>, position: DVec3, value: i32) -> bool {
        let search_box = WorldAabb::new(
            position.x - 0.5,
            position.y - 0.5,
            position.z - 0.5,
            position.x + 0.5,
            position.y + 0.5,
            position.z + 0.5,
        );
        let merge_id = rand::random_range(0..ORB_GROUPS_PER_AREA);
        for entity in world.get_entities_in_aabb(&search_box) {
            let Some(orb) = entity.downcast_ref::<Self>() else {
                continue;
            };
            if !orb.can_merge_id(merge_id, value) {
                continue;
            }

            let mut state = orb.state.lock();
            state.count += 1;
            state.age = 0;
            return true;
        }
        false
    }

    fn scan_for_merges(&self, world: &Arc<World>) {
        let search_box = self.bounding_box().inflate(ORB_MERGE_DISTANCE);
        for entity in world.get_entities_in_aabb(&search_box) {
            if entity.id() == self.id() {
                continue;
            }
            let Some(orb) = entity.downcast_ref::<Self>() else {
                continue;
            };
            if !orb.can_merge_id(self.id(), self.value()) {
                continue;
            }

            self.merge(orb);
            if self.is_removed() {
                return;
            }
        }
    }

    fn can_merge_id(&self, id: i32, value: i32) -> bool {
        !self.is_removed() && (self.id() - id) % ORB_GROUPS_PER_AREA == 0 && self.value() == value
    }

    fn merge(&self, other: &Self) {
        let (other_count, other_age) = {
            let state = other.state.lock();
            (state.count, state.age)
        };
        let mut state = self.state.lock();
        state.count += other_count;
        state.age = state.age.min(other_age);
        other.set_removed(RemovalReason::Discarded);
    }

    fn set_underwater_movement(&self) {
        let velocity = self.velocity();
        self.set_velocity(DVec3::new(
            velocity.x * f64::from(0.99_f32),
            (velocity.y + f64::from(5.0e-4_f32)).min(f64::from(0.06_f32)),
            velocity.z * f64::from(0.99_f32),
        ));
    }

    fn apply_lava_movement(&self, world: &Arc<World>) {
        if !get_fluid_state(world, self.block_position()).is_lava() {
            return;
        }

        let velocity = DVec3::new(
            f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.2),
            f64::from(0.2_f32),
            f64::from((rand::random::<f32>() - rand::random::<f32>()) * 0.2),
        );
        self.set_velocity(velocity);
    }

    fn is_aabb_colliding(&self, world: &Arc<World>, aabb: WorldAabb) -> bool {
        let collision_world = WorldCollisionProvider::for_entity(world, self);
        collision_world.has_entity_context_collision(aabb, self.position().y, self.is_descending())
    }

    fn follow_nearby_player(&self, world: &Arc<World>) {
        let current = self
            .state
            .lock()
            .following_player_id
            .and_then(|id| world.players.get_by_entity_id(id));

        let should_refresh = current.as_ref().is_none_or(|player| {
            player.is_spectator()
                || player.position().distance_squared(self.position()) > MAX_FOLLOW_DIST_SQR
        });

        let following = if should_refresh {
            // A dead nearest player stops the orb rather than passing it on.
            let nearest = world
                .nearest_player(self.position(), MAX_FOLLOW_DIST, |player| {
                    !player.is_spectator()
                })
                .filter(|player| !player.living().is_dead_or_dying());
            self.state.lock().following_player_id = nearest.as_ref().map(|player| player.id());
            nearest
        } else {
            current
        };

        let Some(player) = following else {
            return;
        };

        let player_pos = player.position();
        let delta = DVec3::new(
            player_pos.x - self.position().x,
            player_pos.y + player.eye_height() / 2.0 - self.position().y,
            player_pos.z - self.position().z,
        );
        let power = 1.0 - delta.length_squared().sqrt() / MAX_FOLLOW_DIST;
        self.set_velocity(self.velocity() + vector::normalize(delta) * (power * power * 0.1));
    }

    fn apply_friction_and_bounce(&self, world: &Arc<World>, fall_speed: f64) {
        let mut friction = self.air_drag();
        if self.on_ground()
            && let Some(block_pos) = self.block_pos_below_that_affects_movement()
        {
            friction *= world.get_block_state(block_pos).get_block().config.friction;
        }

        let mut velocity = self.velocity() * f64::from(friction);
        if self.vertical_collision_below() && fall_speed < -self.gravity() {
            velocity.y = -fall_speed * 0.4;
        }
        self.set_velocity(velocity);
    }

    /// Attempts to have a player pick up this experience orb.
    pub fn try_pickup(&self, player: &Arc<Player>) -> bool {
        if player.take_xp_delay() != 0 {
            return false;
        }

        player.set_take_xp_delay(2);
        if let Some(world) = self.level() {
            let take_packet = CTakeItemEntity::new(self.id(), player.id(), 1);
            world.broadcast_to_nearby(
                ChunkPos::from_entity_pos(self.position()),
                take_packet,
                None,
            );
        }

        let remaining = player
            .inventory
            .lock()
            .repair_random_equipped_item_with_xp(self.value());
        if remaining > 0 {
            player.give_experience_points(remaining);
        }

        let remove = {
            let mut state = self.state.lock();
            state.count -= 1;
            state.count == 0
        };
        if remove {
            self.set_removed(RemovalReason::Discarded);
        }
        true
    }
}

impl DefaultEntityPush for ExperienceOrbEntity {}

impl ExperienceOrbEntity {
    const TICK: StageId = StageId("experience_orb.tick");

    const HURT: StageId = StageId("experience_orb.hurt");

    fn define() -> Definition<Self> {
        let definition = Definition::new();
        Definition {
            // `ExperienceOrb.doWaterSplashEffect` does nothing.
            do_water_splash_effect: Pipeline::new([]),
            tick: definition
                .tick
                .then([Stage::effect(Self::TICK, Self::entity_tick)]),
            hurt: Pipeline::new([Stage::new(
                Self::HURT,
                |orb: &Self, ctx: &mut HurtCtx<'_>| {
                    ctx.result = orb.entity_hurt(ctx.world, ctx.source, ctx.amount);
                    Flow::Continue
                },
            )]),
            ..definition
        }
    }

    const ENTITY: EntityProps<Self> = EntityProps {
        is_hard_collision_relevant: |_| false,
        // Vanilla `ExperienceOrb` overrides this with an empty body.
        movement_emission: |_| EntityMovementEmission::None,
        default_gravity: |_| 0.03,
        block_pos_below_that_affects_movement: |entity| entity.on_pos(0.999_999),
        attackable: |_| false,
        sound_source: |_| SoundSource::Ambient,
        player_touch: Self::entity_player_touch,
        ..EntityProps::new()
    };

    /// `ExperienceOrb.tick` after `super.tick()`.
    fn entity_tick(&self) {
        self.set_old_position_to_current();

        let Some(world) = self.level() else {
            return;
        };

        let colliding = self.is_aabb_colliding(&world, self.bounding_box());
        if self.fluid_contact().eye_in_water() {
            self.set_underwater_movement();
        } else if !colliding {
            self.apply_gravity();
        }

        self.apply_lava_movement(&world);

        if self.tick_count() % ENTITY_SCAN_PERIOD == 1 {
            self.scan_for_merges(&world);
            if self.is_removed() {
                return;
            }
        }

        self.follow_nearby_player(&world);
        if self.state.lock().following_player_id.is_none() && colliding {
            let next_colliding =
                self.is_aabb_colliding(&world, self.bounding_box().translate(self.velocity()));
            if next_colliding {
                let bounding_box = self.bounding_box();
                self.move_towards_closest_space(
                    self.position().x,
                    f64::midpoint(bounding_box.min_y(), bounding_box.max_y()),
                    self.position().z,
                );
                self.mark_velocity_sync();
            }
        }

        let fall_speed = self.velocity().y;
        if self
            .move_entity(MoverType::SelfMovement, self.velocity())
            .is_some()
        {
            self.apply_effects_from_blocks();
            if self.is_removed() {
                return;
            }
        }

        self.apply_friction_and_bounce(&world, fall_speed);

        let expired = {
            let mut state = self.state.lock();
            state.age += 1;
            state.age >= LIFETIME
        };
        if expired {
            self.set_removed(RemovalReason::Discarded);
        }
    }

    fn entity_player_touch(&self, player: &Arc<Player>) {
        self.try_pickup(player);
    }

    fn entity_hurt(&self, _world: &World, source: &DamageSource, amount: f32) -> bool {
        if self.is_invulnerable_to_base(source) {
            return false;
        }

        self.mark_hurt();
        let health = {
            let mut state = self.state.lock();
            state.health = (state.health as f32 - amount) as i32;
            state.health
        };
        if health <= 0 {
            self.set_removed(RemovalReason::Discarded);
        }
        true
    }

    fn save_orb(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("Health", state.health as i16);
        nbt.insert("Age", state.age as i16);
        nbt.insert("Value", self.value() as i16);
        nbt.insert("Count", state.count);
    }

    fn load_orb(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        state.health = i32::from(nbt.short_value("Health").unwrap_or(DEFAULT_HEALTH as i16));
        state.age = i32::from(nbt.short_value("Age").unwrap_or(0));
        if let Some(count) = nbt.get("Count").and_then(|tag| tag.codec_i32())
            && count > 0
        {
            state.count = count;
        }
        drop(state);

        self.set_value(i32::from(nbt.short_value("Value").unwrap_or(0)));
    }
}

#[cfg(test)]
mod tests {

    use steel_registry::{init_vanilla_registry, vanilla_damage_types};

    use crate::test_support::test_world;

    use super::*;

    use crate::test_support::load_additional;

    #[test]
    fn experience_value_buckets_match_vanilla() {
        assert_eq!(ExperienceOrbEntity::get_experience_value(2477), 2477);
        assert_eq!(ExperienceOrbEntity::get_experience_value(2476), 1237);
        assert_eq!(ExperienceOrbEntity::get_experience_value(1236), 617);
        assert_eq!(ExperienceOrbEntity::get_experience_value(616), 307);
        assert_eq!(ExperienceOrbEntity::get_experience_value(306), 149);
        assert_eq!(ExperienceOrbEntity::get_experience_value(148), 73);
        assert_eq!(ExperienceOrbEntity::get_experience_value(72), 37);
        assert_eq!(ExperienceOrbEntity::get_experience_value(36), 17);
        assert_eq!(ExperienceOrbEntity::get_experience_value(16), 7);
        assert_eq!(ExperienceOrbEntity::get_experience_value(6), 3);
        assert_eq!(ExperienceOrbEntity::get_experience_value(2), 1);
    }

    #[test]
    fn merge_id_uses_vanilla_grouping_and_value() {
        init_vanilla_registry();

        let orb = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            41,
            DVec3::ZERO,
            Weak::new(),
        );
        orb.set_value(7);

        assert!(orb.can_merge_id(1, 7));
        assert!(!orb.can_merge_id(2, 7));
        assert!(!orb.can_merge_id(1, 3));
    }

    #[test]
    fn experience_orb_merge_absorbs_existing_group() {
        init_vanilla_registry();

        let target = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            41,
            DVec3::ZERO,
            Weak::new(),
        );
        target.set_value(7);
        target.set_age(50);

        let other = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            81,
            DVec3::ZERO,
            Weak::new(),
        );
        other.set_value(7);
        other.set_age(12);
        other.state.lock().count = 3;

        assert!(other.can_merge_id(target.id(), target.value()));
        target.merge(&other);

        assert_eq!(target.count(), 4);
        assert_eq!(target.age(), 12);
        assert!(other.is_removed());
    }

    #[test]
    fn orb_damage_truncates_after_fractional_subtraction() {
        init_vanilla_registry();

        let orb = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            1,
            DVec3::ZERO,
            Weak::new(),
        );

        assert!(orb.hurt(
            test_world(),
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            0.75,
        ));

        assert_eq!(orb.health(), 4);
    }

    #[test]
    fn orb_saves_and_loads_vanilla_state() {
        init_vanilla_registry();

        let orb = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            1,
            DVec3::ZERO,
            Weak::new(),
        );
        orb.set_value(17);
        orb.set_age(42);
        {
            let mut state = orb.state.lock();
            state.health = 3;
            state.count = 4;
        }

        let mut nbt = NbtCompound::new();
        orb.save_additional(&mut nbt);

        let loaded = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        load_additional(&*loaded, &nbt);

        assert_eq!(loaded.value(), 17);
        assert_eq!(loaded.age(), 42);
        assert_eq!(loaded.health(), 3);
        assert_eq!(loaded.count(), 4);
    }
}
