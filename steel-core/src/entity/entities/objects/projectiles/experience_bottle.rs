//! Thrown Bottle o' Enchanting projectile entity (`ThrownExperienceBottle`).
//!
//! Mirrors vanilla `ThrownExperienceBottle` on the Steel
//! `Projectile → ThrowableProjectile → ThrowableItemProjectile` trait stack.
//! It falls faster than other thrown items, deals no damage, and on impact
//! plays the potion splash level event and awards 3 to 11 experience as orbs
//! biased away from the surface it hit.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_entity_data::ExperienceBottleEntityData;
use steel_registry::{level_events, vanilla_items};
use steel_utils::BlockPos;
use steel_utils::locks::SyncMutex;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, Projectile, ProjectileBase,
    ProjectileHit, RemovalReason, SharedEntity, ThrowableItemProjectile, ThrowableProjectile,
};
use crate::world::World;

/// Vanilla `ThrownExperienceBottle.getDefaultGravity`.
const GRAVITY: f64 = 0.07;
/// Vanilla `ThrownExperienceBottle.onHit` level event data: the water potion color.
const WATER_POTION_COLOR: i32 = -13_083_194;
/// Vanilla base experience per bottle before the two `nextInt(5)` rolls.
const BASE_EXPERIENCE: i32 = 3;
/// Vanilla `nextInt(5)` bound for each of the two experience rolls.
const EXPERIENCE_ROLL_BOUND: i32 = 5;

/// A thrown Bottle o' Enchanting.
#[entity_behavior(class = "ThrownExperienceBottle")]
pub struct ThrownExperienceBottleEntity {
    /// Common entity fields (id, uuid, position, etc.).
    base: EntityBase,
    /// Vanilla entity type registered for this implementation.
    entity_type: EntityTypeRef,
    /// Synced data carrying the rendered item stack.
    entity_data: SyncMutex<ExperienceBottleEntityData>,
    /// Shared `Projectile` state (owner / left-owner / has-been-shot).
    projectile_base: ProjectileBase,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ThrownExperienceBottleEntity`.
unsafe impl DowncastType for ThrownExperienceBottleEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/experience_bottle");
}

impl ThrownExperienceBottleEntity {
    /// Creates a new thrown bottle with no owner and the default rendered item.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self {
            base: EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
            entity_data: SyncMutex::new(ExperienceBottleEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Creates a thrown bottle from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self {
            base: EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
            entity_data: SyncMutex::new(ExperienceBottleEntityData::new()),
            projectile_base: ProjectileBase::new(),
        }
    }

    /// Vanilla `3 + random.nextInt(5) + random.nextInt(5)` with the two rolls supplied.
    const fn experience_amount(first_roll: i32, second_roll: i32) -> i32 {
        BASE_EXPERIENCE + first_roll + second_roll
    }
}

impl Entity for ThrownExperienceBottleEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn tick(&self) {
        self.throwable_projectile_tick();
    }

    /// Vanilla `ThrownExperienceBottle.getDefaultGravity` (0.07, heavier than other thrown items).
    fn get_default_gravity(&self) -> f64 {
        GRAVITY
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_projectile(nbt);
        self.save_throwable_item(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_projectile(nbt);
        self.load_throwable_item(nbt);
    }
}

impl Projectile for ThrownExperienceBottleEntity {
    fn projectile_base(&self) -> &ProjectileBase {
        &self.projectile_base
    }

    /// Vanilla `ThrownExperienceBottle.onHit`: splash event, experience orbs
    /// biased away from the hit surface, then discard. No entity damage.
    fn on_hit(&self, hit: &ProjectileHit) {
        self.projectile_on_hit(hit);

        let Some(world) = self.level() else {
            return;
        };

        // VANILLA CLIENT-LOCAL: level event 2002 renders the splash particles on
        // clients; the server only relays it with the water potion color.
        world.level_event(
            level_events::PARTICLES_SPELL_POTION_SPLASH,
            BlockPos::from(self.position()),
            WATER_POTION_COLOR,
            None,
        );

        let experience = Self::experience_amount(
            rand::random_range(0..EXPERIENCE_ROLL_BOUND),
            rand::random_range(0..EXPERIENCE_ROLL_BOUND),
        );
        let rough_direction = match hit {
            ProjectileHit::Block { hit, .. } => hit.direction.offset_vec().as_dvec3(),
            ProjectileHit::Entity(_) => -self.velocity(),
        };
        ExperienceOrbEntity::award_with_direction(
            &world,
            hit.location(),
            rough_direction,
            experience,
        );

        self.set_removed(RemovalReason::Discarded);
    }
}

impl ThrowableProjectile for ThrownExperienceBottleEntity {}

impl ThrowableItemProjectile for ThrownExperienceBottleEntity {
    fn get_default_item(&self) -> ItemRef {
        &vanilla_items::EXPERIENCE_BOTTLE
    }

    fn set_item(&self, item: ItemStack) {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .set(item);
    }

    fn get_item(&self) -> ItemStack {
        self.entity_data
            .lock()
            .throwable_item_projectile
            .item_stack
            .get()
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities, vanilla_items};
    use steel_utils::{BlockPos, ChunkPos, Direction, Downcast, WorldAabb};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::entity::entities::ExperienceOrbEntity;
    use crate::entity::{
        Entity, EntityHitResult, Projectile, ProjectileHit, SharedEntity, ThrowableItemProjectile,
    };
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use crate::world::{ClipHitResult, World};

    fn orb_values_and_velocities(world: &Arc<World>, around: DVec3) -> Vec<(i32, DVec3)> {
        world
            .get_entities_in_aabb(&WorldAabb::of_size(around, 3.0, 3.0, 3.0))
            .iter()
            .filter_map(|entity| {
                entity
                    .as_ref()
                    .downcast_ref::<ExperienceOrbEntity>()
                    .map(|orb| (orb.value(), orb.velocity()))
            })
            .collect()
    }

    #[test]
    fn defaults_match_vanilla() {
        init_vanilla_registry();
        let bottle = ThrownExperienceBottleEntity::new(
            &vanilla_entities::EXPERIENCE_BOTTLE,
            1,
            DVec3::ZERO,
            Weak::<World>::new(),
        );
        assert!((bottle.get_default_gravity() - 0.07).abs() < f64::EPSILON);
        assert!(bottle.get_item().is(&vanilla_items::EXPERIENCE_BOTTLE));
        assert!(!bottle.attackable());
    }

    #[test]
    fn experience_amount_covers_the_vanilla_range() {
        assert_eq!(ThrownExperienceBottleEntity::experience_amount(0, 0), 3);
        assert_eq!(ThrownExperienceBottleEntity::experience_amount(4, 4), 11);
        assert_eq!(ThrownExperienceBottleEntity::experience_amount(2, 1), 6);
    }

    #[test]
    fn hitting_a_block_awards_experience_away_from_the_face_and_discards() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("experience_bottle_block_hit");
        let location = DVec3::new(0.5, 80.0, 0.5);
        insert_ready_full_chunk(&world, ChunkPos::from_entity_pos(location));
        let bottle = ThrownExperienceBottleEntity::new(
            &vanilla_entities::EXPERIENCE_BOTTLE,
            1,
            location,
            Arc::downgrade(&world),
        );
        // A downward-facing block face (the underside of an overhang) forces the
        // orbs' random upward motion to be flipped, so the bias is observable.
        let hit = ProjectileHit::Block {
            location,
            hit: ClipHitResult {
                location,
                direction: Direction::Down,
                block_pos: BlockPos::new(0, 81, 0),
                miss: false,
                inside: false,
                world_border_hit: false,
            },
        };

        bottle.on_hit(&hit);

        assert!(bottle.is_removed());
        let orbs = orb_values_and_velocities(&world, location);
        let total: i32 = orbs.iter().map(|(value, _)| value).sum();
        assert!((3..=11).contains(&total), "total {total}");
        assert!(
            orbs.iter().all(|(_, velocity)| velocity.y <= 0.0),
            "orbs fly away from a downward face"
        );
    }

    #[test]
    fn hitting_an_entity_awards_experience_back_along_the_flight_path() {
        init_vanilla_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("experience_bottle_entity_hit");
        let location = DVec3::new(0.5, 80.0, 0.5);
        insert_ready_full_chunk(&world, ChunkPos::from_entity_pos(location));
        let bottle = ThrownExperienceBottleEntity::new(
            &vanilla_entities::EXPERIENCE_BOTTLE,
            1,
            location,
            Arc::downgrade(&world),
        );
        // Flying upward: the reversed flight path points down, forcing the flip.
        bottle.set_velocity(DVec3::new(0.0, 1.0, 0.0));
        let target: SharedEntity = Arc::new(ThrownExperienceBottleEntity::new(
            &vanilla_entities::EXPERIENCE_BOTTLE,
            2,
            location,
            Arc::downgrade(&world),
        ));

        bottle.on_hit(&ProjectileHit::Entity(EntityHitResult {
            entity: target,
            location,
        }));

        assert!(bottle.is_removed());
        let orbs = orb_values_and_velocities(&world, location);
        assert_ne!(orbs.len(), 0);
        assert!(
            orbs.iter().all(|(_, velocity)| velocity.y <= 0.0),
            "reversed upward flight points down"
        );
    }
}
