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
use steel_utils::random::Random as _;
use steel_utils::{BlockPos, ChunkPos, WorldAabb};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, RemovalReason,
    SharedEntity, next_entity_id,
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

/// Vanilla experience orb entity.
#[entity_behavior(class = "experience_orb")]
pub struct ExperienceOrb {
    base: Weak<EntityBase>,
    entity_data: ExperienceOrbEntityData,
    age: i32,
    health: i32,
    count: i32,
    following_player_id: Option<i32>,
}

impl ExperienceOrb {
    fn build(base: Weak<EntityBase>) -> Self {
        Self {
            base,

            entity_data: ExperienceOrbEntityData::new(),
            age: 0,
            health: DEFAULT_HEALTH,
            count: 1,
            following_player_id: None,
        }
    }

    /// Creates a new experience orb with value 0.
    #[must_use]
    pub fn new(id: i32, position: DVec3, world: Weak<World>) -> SharedEntity {
        EntityBase::pack_with(
            id,
            position,
            vanilla_entities::EXPERIENCE_ORB.dimensions,
            world,
            |base| Self::build(base),
        )
    }

    /// Creates a new experience orb with a value and vanilla spawn motion.
    #[must_use]
    pub fn with_value(position: DVec3, value: i32, world: Weak<World>) -> SharedEntity {
        let entity = EntityBase::pack_with(
            next_entity_id(),
            position,
            vanilla_entities::EXPERIENCE_ORB.dimensions,
            world,
            |base| Self::build(base),
        );

        {
            let mut entity = entity.lock_entity();
            let exp_orb: &mut Self = entity.downcast().unwrap();

            exp_orb.set_value(value);
            exp_orb.initialize_spawn_movement();
        }

        entity
    }

    /// Creates an experience orb from saved base data.
    #[must_use]
    pub fn from_saved(load: EntityBaseLoad) -> SharedEntity {
        EntityBase::pack_loaded_with(load, vanilla_entities::EXPERIENCE_ORB.dimensions, |base| {
            Self::build(base)
        })
    }

    /// Spawns vanilla experience orbs for an XP amount.
    pub fn award(world: &Arc<World>, position: DVec3, mut amount: i32) {
        while amount > 0 {
            let value = Self::get_experience_value(amount);
            amount -= value;
            if Self::try_merge_to_existing(world, position, value) {
                continue;
            }

            let entity: SharedEntity = Self::with_value(position, value, Arc::downgrade(world));
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
        *self.entity_data.value.get()
    }

    /// Sets this orb's XP value.
    pub fn set_value(&mut self, value: i32) {
        self.entity_data.set_value(value);
    }

    fn initialize_spawn_movement(&self) {
        let (yaw, velocity) = {
            let base = self.base();
            let mut random = base.random().lock();
            let yaw = random.next_f32() * 360.0;
            let velocity = DVec3::new(
                (f64::from(random.next_f32()) * 0.2 - 0.1) * 2.0,
                f64::from(random.next_f32()) * 0.2 * 2.0,
                (f64::from(random.next_f32()) * 0.2 - 0.1) * 2.0,
            );
            (yaw, velocity)
        };
        self.set_rotation((yaw, 0.0));
        self.set_velocity(velocity);
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
        let merge_id = world.random().lock().next_i32_bounded(ORB_GROUPS_PER_AREA);
        for entity in world.get_entities_in_aabb(&search_box) {
            // Filter by type lock-free before locking: `with_entity_as` locks the
            // entity's behavior mutex, and the AABB includes the dying entity whose
            // death triggered this award (already locked) — locking it again would
            // deadlock. Only actual orbs need locking.
            if entity.entity_type() != &vanilla_entities::EXPERIENCE_ORB {
                continue;
            }
            let merged = entity
                .with_entity_as::<Self, _>(|orb| {
                    if !orb.can_merge_id(merge_id, value) {
                        return false;
                    }
                    orb.count += 1;
                    orb.age = 0;
                    true
                })
                .unwrap_or(false);
            if merged {
                return true;
            }
        }
        false
    }

    fn scan_for_merges(&mut self, world: &Arc<World>) {
        let search_box = self.bounding_box().inflate(ORB_MERGE_DISTANCE);
        for entity in world.get_entities_in_aabb(&search_box) {
            if entity.id() == self.id() {
                continue;
            }
            // Skip non-orbs lock-free; see `try_merge_to_existing`.
            if entity.entity_type() != &vanilla_entities::EXPERIENCE_ORB {
                continue;
            }
            entity.with_entity_as::<Self, _>(|orb| {
                if orb.can_merge_id(self.id(), self.value()) {
                    self.merge(orb);
                }
            });
            if self.is_removed() {
                return;
            }
        }
    }

    fn can_merge_id(&self, id: i32, value: i32) -> bool {
        !self.is_removed() && (self.id() - id) % ORB_GROUPS_PER_AREA == 0 && self.value() == value
    }

    fn merge(&mut self, other: &ExperienceOrb) {
        self.count += other.count;
        self.age = self.age.min(other.age);

        other.set_removed(RemovalReason::Discarded);
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

        let velocity = {
            let base = self.base();
            let mut random = base.random().lock();
            DVec3::new(
                f64::from(random.next_f32() - random.next_f32()) * 0.2,
                0.2,
                f64::from(random.next_f32() - random.next_f32()) * 0.2,
            )
        };
        self.set_velocity(velocity);
    }

    fn is_aabb_colliding(&self, world: &Arc<World>, aabb: WorldAabb) -> bool {
        let collision_world = WorldCollisionProvider::for_entity(world, self);
        collision_world.has_entity_context_collision(aabb, self.position().y, self.is_descending())
    }

    fn follow_nearby_player(&mut self, world: &Arc<World>) {
        let current = self
            .following_player_id
            .and_then(|id| world.players.get_by_entity_id(id))
            .map(|sp| sp.entity.clone());

        let should_refresh = current.as_ref().is_none_or(|player| {
            let player = player.lock();

            player.is_spectator()
                || player.is_dead_or_dying()
                || player.position().distance_squared(self.position()) > MAX_FOLLOW_DIST_SQR
        });

        let following = if should_refresh {
            let nearest = world.nearest_player(self.position(), MAX_FOLLOW_DIST, |player| {
                !player.is_spectator() && !player.is_dead_or_dying()
            });
            self.following_player_id = nearest.as_ref().map(|player| player.lock().id());
            nearest
        } else {
            current
        };

        let Some(player) = following else {
            return;
        };
        let player = player.lock();

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
                .map(|block_pos| {
                    f64::from(world.get_block_state(block_pos).get_block().config.friction)
                        * AIR_FRICTION
                })
                .unwrap_or(AIR_FRICTION)
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
    pub fn try_pickup(&mut self, player: &mut Player) -> bool {
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

        let remaining = {
            let mut inventory = player.inventory.lock();
            let player_base = player.base();
            let mut random = player_base.random().lock();
            inventory.repair_random_equipped_item_with_xp(self.value(), &mut *random)
        };
        if remaining > 0 {
            player.give_experience_points(remaining);
        }

        let remove = {
            self.count -= 1;
            self.count == 0
        };
        if remove {
            self.set_removed(RemovalReason::Discarded);
        }
        true
    }
}

impl Entity for ExperienceOrb {
    fn base_weak(&self) -> &Weak<EntityBase> {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        &vanilla_entities::EXPERIENCE_ORB
    }

    fn tick(&mut self) {
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
        if self.following_player_id.is_none() && colliding {
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
            self.age += 1;
            self.age >= LIFETIME
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

    fn synced_data_mut(&mut self) -> Option<&mut dyn EntitySyncedData> {
        Some(&mut self.entity_data)
    }

    fn player_touch(&mut self, player: &mut Player) {
        self.try_pickup(player);
    }

    fn hurt(&mut self, source: &DamageSource, amount: f32) -> bool {
        if self.is_base_invulnerable_to(source) {
            return false;
        }

        self.mark_hurt();
        let health = {
            self.health = (self.health as f32 - amount) as i32;
            self.health
        };
        if health <= 0 {
            self.set_removed(RemovalReason::Discarded);
        }
        true
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("Health", self.health as i16);
        nbt.insert("Age", self.age as i16);
        nbt.insert("Value", self.value() as i16);
        nbt.insert("Count", self.count);
    }

    fn load_additional(&mut self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.health = i32::from(nbt.short("Health").unwrap_or(DEFAULT_HEALTH as i16));
        self.age = i32::from(nbt.short("Age").unwrap_or(0));
        if let Some(count) = nbt.int("Count")
            && count > 0
        {
            self.count = count;
        }

        self.set_value(i32::from(nbt.short("Value").unwrap_or(0)));
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
        assert_eq!(ExperienceOrb::get_experience_value(2477), 2477);
        assert_eq!(ExperienceOrb::get_experience_value(2476), 1237);
        assert_eq!(ExperienceOrb::get_experience_value(1236), 617);
        assert_eq!(ExperienceOrb::get_experience_value(616), 307);
        assert_eq!(ExperienceOrb::get_experience_value(306), 149);
        assert_eq!(ExperienceOrb::get_experience_value(148), 73);
        assert_eq!(ExperienceOrb::get_experience_value(72), 37);
        assert_eq!(ExperienceOrb::get_experience_value(36), 17);
        assert_eq!(ExperienceOrb::get_experience_value(16), 7);
        assert_eq!(ExperienceOrb::get_experience_value(6), 3);
        assert_eq!(ExperienceOrb::get_experience_value(2), 1);
    }

    fn test_orb() -> ExperienceOrb {
        // Fixed id 1: merge grouping asserts depend on `id % ORB_GROUPS_PER_AREA`.
        let base = Arc::new(EntityBase::new(
            1,
            DVec3::ZERO,
            vanilla_entities::EXPERIENCE_ORB.dimensions,
            Weak::new(),
        ));
        let base_weak = Arc::downgrade(&base);
        // Leak the base so the weak back-reference stays upgradable.
        std::mem::forget(base);
        ExperienceOrb::build(base_weak)
    }

    #[test]
    fn merge_id_uses_vanilla_grouping_and_value() {
        init_test_registry();

        let mut orb = test_orb();
        orb.set_value(7);

        assert!(orb.can_merge_id(1, 7));
        assert!(!orb.can_merge_id(2, 7));
        assert!(!orb.can_merge_id(1, 3));
    }

    #[test]
    fn orb_damage_truncates_after_fractional_subtraction() {
        init_test_registry();

        let mut orb = test_orb();

        assert!(orb.hurt(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            0.75,
        ));

        assert_eq!(orb.health, 4);
    }

    #[test]
    fn orb_saves_and_loads_vanilla_state() {
        init_test_registry();

        let mut orb = test_orb();
        orb.set_value(17);
        orb.age = 42;
        orb.health = 3;
        orb.count = 4;

        let mut nbt = NbtCompound::new();
        orb.save_additional(&mut nbt);

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let mut loaded = test_orb();
        loaded.load_additional((&borrowed).into());

        assert_eq!(loaded.value(), 17);
        assert_eq!(loaded.age, 42);
        assert_eq!(loaded.health, 3);
        assert_eq!(loaded.count, 4);
    }
}
