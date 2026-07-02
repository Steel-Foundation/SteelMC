//! Experience orb entity implementation.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{CTakeItemEntity, SoundSource};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::fluid::FluidStateExt as _;
use steel_registry::vanilla_entity_data::ExperienceOrbEntityData;
use steel_registry::{vanilla_damage_type_tags, vanilla_entities};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, ChunkPos, WorldAabb};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityCapabilities, EntitySyncedData,
    ExperienceOrbMergeEntity, LivingEntity, RemovalReason, SharedEntity, next_entity_id,
};
use crate::fluid::get_fluid_state;
use crate::physics::{MoverType, WorldCollisionProvider};
use crate::player::Player;
use crate::world::World;

const LIFETIME: i32 = 6000;
const ENTITY_SCAN_PERIOD: i32 = 20;
const MAX_FOLLOW_DIST: f64 = 8.0;
const MAX_FOLLOW_DIST_SQR: f64 = MAX_FOLLOW_DIST * MAX_FOLLOW_DIST;
const ORB_GROUPS_PER_AREA: i32 = 40;
const ORB_MERGE_DISTANCE: f64 = 0.5;
const DEFAULT_HEALTH: i32 = 5;
const DEFAULT_GRAVITY: f64 = 0.03;
const AIR_FRICTION: f64 = 0.98;
const BOUNCE_SCALE: f64 = 0.4;
const UNDERWATER_DRAG: f64 = 0.99;
const UNDERWATER_VERTICAL_ACCEL: f64 = 5.0e-4;
const UNDERWATER_MAX_Y: f64 = 0.06;
const FOLLOW_ACCELERATION: f64 = 0.1;

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
pub struct ExperienceOrbEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    entity_data: SyncMutex<ExperienceOrbEntityData>,
    state: SyncMutex<ExperienceOrbState>,
}

impl ExperienceOrbEntity {
    /// Creates a new experience orb with value 0.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(ExperienceOrbEntityData::new()),
            state: SyncMutex::new(ExperienceOrbState::new()),
        }
    }

    /// Creates a new experience orb with a value and vanilla spawn motion.
    #[must_use]
    pub fn with_value(
        entity_type: EntityTypeRef,
        id: i32,
        position: DVec3,
        value: i32,
        world: Weak<World>,
    ) -> Self {
        let entity = Self::new(entity_type, id, position, world);
        entity.set_value(value);
        entity.initialize_spawn_movement();
        entity
    }

    /// Creates an experience orb from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ExperienceOrbEntityData::new()),
            state: SyncMutex::new(ExperienceOrbState::new()),
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

            let entity: SharedEntity = Arc::new(Self::with_value(
                &vanilla_entities::EXPERIENCE_ORB,
                next_entity_id(),
                position,
                value,
                Arc::downgrade(world),
            ));
            if let Err(error) = world.try_add_entity(entity) {
                log::debug!("failed to add experience orb: {error}");
            }
        }
    }

    /// Vanilla `ExperienceOrb.getExperienceValue`.
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
        let yaw = rand::random::<f32>() * 360.0;
        let velocity = DVec3::new(
            (f64::from(rand::random::<f32>()) * 0.2 - 0.1) * 2.0,
            f64::from(rand::random::<f32>()) * 0.2 * 2.0,
            (f64::from(rand::random::<f32>()) * 0.2 - 0.1) * 2.0,
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
            let Some(orb) = entity.as_experience_orb_merge_entity() else {
                continue;
            };
            if orb.try_absorb_awarded_experience_orb(merge_id, value) {
                return true;
            }
        }
        false
    }

    fn scan_for_merges(&self, world: &Arc<World>) {
        let search_box = self.bounding_box().inflate(ORB_MERGE_DISTANCE);
        for entity in world.get_entities_in_aabb(&search_box) {
            if entity.id() == self.id() {
                continue;
            }
            let Some(orb) = entity.as_experience_orb_merge_entity() else {
                continue;
            };
            self.try_absorb_experience_orb(orb, self.id(), self.value());
            if self.is_removed() {
                return;
            }
        }
    }

    fn can_merge_id(&self, id: i32, value: i32) -> bool {
        !self.is_removed() && (self.id() - id) % ORB_GROUPS_PER_AREA == 0 && self.value() == value
    }

    fn set_underwater_movement(&self) {
        let velocity = self.velocity();
        self.set_velocity(DVec3::new(
            velocity.x * UNDERWATER_DRAG,
            (velocity.y + UNDERWATER_VERTICAL_ACCEL).min(UNDERWATER_MAX_Y),
            velocity.z * UNDERWATER_DRAG,
        ));
    }

    fn apply_lava_movement(&self, world: &Arc<World>) {
        if !get_fluid_state(world, self.block_position()).is_lava() {
            return;
        }

        let velocity = DVec3::new(
            f64::from(rand::random::<f32>() - rand::random::<f32>()) * 0.2,
            0.2,
            f64::from(rand::random::<f32>() - rand::random::<f32>()) * 0.2,
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
                || player.is_dead_or_dying()
                || player.position().distance_squared(self.position()) > MAX_FOLLOW_DIST_SQR
        });

        let following = if should_refresh {
            let nearest = world.nearest_player(self.position(), MAX_FOLLOW_DIST, |player| {
                !player.is_spectator() && !player.is_dead_or_dying()
            });
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
            player_pos.y + player.get_eye_height() / 2.0 - self.position().y,
            player_pos.z - self.position().z,
        );
        let length_sqr = delta.length_squared();
        if length_sqr <= f64::EPSILON {
            return;
        }

        let power = 1.0 - length_sqr.sqrt() / MAX_FOLLOW_DIST;
        self.set_velocity(
            self.velocity() + delta.normalize() * (power * power * FOLLOW_ACCELERATION),
        );
    }

    fn apply_friction_and_bounce(&self, world: &Arc<World>, fall_speed: f64) {
        let friction = if self.on_ground() {
            self.block_pos_below_that_affects_movement()
                .map_or(AIR_FRICTION, |block_pos| {
                    f64::from(world.get_block_state(block_pos).get_block().config.friction)
                        * AIR_FRICTION
                })
        } else {
            AIR_FRICTION
        };

        let mut velocity = self.velocity() * friction;
        if self.vertical_collision_below() && fall_speed < -self.get_gravity() {
            velocity.y = -fall_speed * BOUNCE_SCALE;
        }
        self.set_velocity(velocity);
    }

    fn is_base_invulnerable_to(&self, source: &DamageSource) -> bool {
        self.is_removed()
            || self.is_invulnerable() && !source.bypasses_invulnerability()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FIRE) && self.fire_immune()
            || source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_FALL)
                && self.is_fall_damage_immune()
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

impl Entity for ExperienceOrbEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.default_tick();
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

    fn get_default_gravity(&self) -> f64 {
        DEFAULT_GRAVITY
    }

    fn block_pos_below_that_affects_movement(&self) -> Option<BlockPos> {
        self.on_pos(0.999_999)
    }

    fn attackable(&self) -> bool {
        false
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Ambient
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn capabilities(&self) -> EntityCapabilities<'_> {
        EntityCapabilities::none().with_experience_orb_merge_entity(self)
    }

    fn player_touch(self: Arc<Self>, player: &Arc<Player>) {
        self.try_pickup(player);
    }

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        if self.is_base_invulnerable_to(source) {
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let state = self.state.lock();
        nbt.insert("Health", state.health as i16);
        nbt.insert("Age", state.age as i16);
        nbt.insert("Value", self.value() as i16);
        nbt.insert("Count", state.count);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        state.health = i32::from(nbt.short("Health").unwrap_or(DEFAULT_HEALTH as i16));
        state.age = i32::from(nbt.short("Age").unwrap_or(0));
        if let Some(count) = nbt.int("Count")
            && count > 0
        {
            state.count = count;
        }
        drop(state);

        self.set_value(i32::from(nbt.short("Value").unwrap_or(0)));
    }
}

impl ExperienceOrbMergeEntity for ExperienceOrbEntity {
    fn can_merge_experience_orb(&self, merge_id: i32, value: i32) -> bool {
        self.can_merge_id(merge_id, value)
    }

    fn try_absorb_awarded_experience_orb(&self, merge_id: i32, value: i32) -> bool {
        if !self.can_merge_experience_orb(merge_id, value) {
            return false;
        }

        let mut state = self.state.lock();
        state.count += 1;
        state.age = 0;
        true
    }

    fn try_absorb_experience_orb(
        &self,
        other: &dyn ExperienceOrbMergeEntity,
        merge_id: i32,
        value: i32,
    ) -> bool {
        if !other.can_merge_experience_orb(merge_id, value) {
            return false;
        }

        let (other_count, other_age) = other.experience_orb_merge_state();
        let mut state = self.state.lock();
        state.count += other_count;
        state.age = state.age.min(other_age);
        other.set_removed(RemovalReason::Discarded);
        true
    }

    fn experience_orb_merge_state(&self) -> (i32, i32) {
        let state = self.state.lock();
        (state.count, state.age)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use steel_registry::{test_support::init_test_registry, vanilla_damage_types};

    use super::*;

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
        init_test_registry();

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
    fn experience_orb_merge_capability_absorbs_existing_group() {
        init_test_registry();

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

        let Some(other_merge) = other.as_experience_orb_merge_entity() else {
            panic!("experience orb should expose merge capability");
        };
        assert!(target.try_absorb_experience_orb(other_merge, target.id(), target.value()));

        assert_eq!(target.count(), 4);
        assert_eq!(target.age(), 12);
        assert!(other.is_removed());
    }

    #[test]
    fn orb_damage_truncates_after_fractional_subtraction() {
        init_test_registry();

        let orb = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            1,
            DVec3::ZERO,
            Weak::new(),
        );

        assert!(orb.hurt(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            0.75,
        ));

        assert_eq!(orb.health(), 4);
    }

    #[test]
    fn orb_saves_and_loads_vanilla_state() {
        init_test_registry();

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

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let loaded = ExperienceOrbEntity::new(
            &vanilla_entities::EXPERIENCE_ORB,
            2,
            DVec3::ZERO,
            Weak::new(),
        );
        loaded.load_additional((&borrowed).into());

        assert_eq!(loaded.value(), 17);
        assert_eq!(loaded.age(), 42);
        assert_eq!(loaded.health(), 3);
        assert_eq!(loaded.count(), 4);
    }
}
