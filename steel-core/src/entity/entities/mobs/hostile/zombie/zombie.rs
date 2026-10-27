//! Vanilla `Zombie`.

use super::{
    Arc, BlockPos, BlockStateId, BorrowedNbtCompoundView, ConversionTracker, DEFAULT_STEP_HEIGHT,
    DROWNED_CONVERSION_TIME, DVec3, DamageSource, DifficultyInstance, DoorBreakingState, Entity,
    EntityBase, EntityBaseLoad, EntityDimensions, EntityEventSource, EntityPose, EntitySpawnReason,
    EntitySyncedData, EntityTypeRef, IN_WATER_TIME, ItemStack, LivingEntity, LivingEntityBase,
    MappedMutexGuard, Mob, MobBase, Monster, MutexGuard, NbtCompound, PathfinderMob, Player,
    SharedEntity, SoundEventRef, SoundSource, SpawnGroupData, SyncMutex, Weak, World, XP_REWARD,
    Zombie, ZombieEntityData, baby_dimensions, register_zombie_goals, vanilla_attributes,
};

const BABY_EYE_HEIGHT: f32 = 0.775;
const BABY_DIMENSIONS: EntityDimensions = baby_dimensions(BABY_EYE_HEIGHT);

/// Vanilla Zombie.
#[steel_macros::entity_behavior(class = "Zombie")]
pub struct ZombieEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<ZombieEntityData>,
    door_breaking: SyncMutex<DoorBreakingState>,
    conversion_tracker: SyncMutex<ConversionTracker>,
}

// SAFETY: The owner-scoped type key uniquely identifies ZombieEntity.
unsafe impl steel_utils::DowncastType for ZombieEntity {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:entity/zombie");
}

impl ZombieEntity {
    /// Creates a new zombie entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved zombie entity.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        mob_base.set_xp_reward(XP_REWARD);
        let mut entity_data = ZombieEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        register_zombie_goals(&mob_base);

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            door_breaking: SyncMutex::new(DoorBreakingState::default()),
            conversion_tracker: SyncMutex::new(ConversionTracker::new(
                IN_WATER_TIME,
                DROWNED_CONVERSION_TIME,
            )),
        }
    }
}

// TODO(villager_infection): override killed_entity to convert a killed Villager
// into a Zombie Villager on Normal/Hard once both entities exist.
// TODO(reinforcements): override hurt_server to spawn backup zombies on Hard once
// SpawnPlacements and the entity AABB/liquid helpers exist.

impl_zombie_state!(ZombieEntity, BABY_DIMENSIONS, {});
impl_zombie_entity!(ZombieEntity, {});
impl_zombie_living!(ZombieEntity, {});
impl_zombie_mob!(ZombieEntity, {});

impl Monster for ZombieEntity {}

impl PathfinderMob for ZombieEntity {}
