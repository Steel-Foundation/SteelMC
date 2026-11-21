//! Vanilla `Husk`.

use steel_registry::vanilla_mob_effects;

use super::{
    Arc, BlockPos, BlockStateId, BorrowedNbtCompoundView, ConversionTracker, DEFAULT_STEP_HEIGHT,
    DROWNED_CONVERSION_TIME, DVec3, DamageSource, DifficultyInstance, DoorBreakingState, Entity,
    EntityBase, EntityBaseLoad, EntityDimensions, EntityEventSource, EntityPose, EntitySpawnReason,
    EntitySyncedData, EntityTypeRef, EquipmentSlot, HuskEntityData, IN_WATER_TIME, ItemStack,
    LivingEntity, LivingEntityBase, MappedMutexGuard, Mob, MobBase, MobEffectInstance, Monster,
    MutexGuard, NbtCompound, PICKUP_LOOT_CHANCE, PathfinderMob, Player, SharedEntity,
    SoundEventRef, SoundSource, SpawnGroupData, SyncMutex, Weak, World, XP_REWARD, Zombie,
    ZombieEntityData, baby_dimensions, level_events, register_zombie_goals, sound_events,
    vanilla_attributes, vanilla_entities,
};

const BABY_EYE_HEIGHT: f32 = 0.825;
const BABY_DIMENSIONS: EntityDimensions = baby_dimensions(BABY_EYE_HEIGHT);
const HUNGER_DURATION_PER_DIFFICULTY: i32 = 140;

/// Vanilla Husk.
#[steel_macros::entity_behavior(class = "Husk")]
pub struct HuskEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<HuskEntityData>,
    door_breaking: SyncMutex<DoorBreakingState>,
    conversion_tracker: SyncMutex<ConversionTracker>,
}

// SAFETY: The owner-scoped type key uniquely identifies HuskEntity.
unsafe impl steel_utils::DowncastType for HuskEntity {
    const TYPE_KEY: steel_utils::DowncastTypeKey =
        steel_utils::DowncastTypeKey::new("steel:entity/husk");
}

impl HuskEntity {
    /// Creates a new husk entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved husk entity.
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
        let mut entity_data = HuskEntityData::new();
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

impl_zombie_state!(HuskEntity, BABY_DIMENSIONS, {
    fn ambient_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_HUSK_AMBIENT
    }

    fn hurt_sound_zombie(&self, _source: &DamageSource) -> SoundEventRef {
        &sound_events::ENTITY_HUSK_HURT
    }

    fn death_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_HUSK_DEATH
    }

    fn step_sound_zombie(&self) -> SoundEventRef {
        &sound_events::ENTITY_HUSK_STEP
    }

    fn converts_to_when_drowning(&self) -> EntityTypeRef {
        &vanilla_entities::ZOMBIE
    }

    fn conversion_level_event(&self) -> i32 {
        level_events::SOUND_HUSK_TO_ZOMBIE
    }

    fn do_hurt_target_zombie(
        &self,
        entity: &SharedEntity,
        world: &World,
        target: &SharedEntity,
    ) -> bool {
        let result = self.do_hurt_target_zombie_default(entity, world, target);
        if result && self.equipment_in_slot(EquipmentSlot::MainHand).is_empty() {
            let difficulty = world
                .get_current_difficulty_at(self.block_position())
                .effective_difficulty();
            if let Some(living) = target.as_living_entity() {
                living.add_mob_effect_with_source(
                    MobEffectInstance::with_duration(
                        vanilla_mob_effects::HUNGER,
                        HUNGER_DURATION_PER_DIFFICULTY * difficulty as i32,
                        0,
                    ),
                    Some(entity.as_ref()),
                );
            }
        }
        result
    }

    fn finalize_spawn_zombie(
        &self,
        world: &Arc<World>,
        difficulty: DifficultyInstance,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let group_data =
            self.finalize_spawn_zombie_default(world, difficulty, spawn_reason, group_data);
        let difficulty_modifier = difficulty.special_multiplier();

        // Vanilla rolls the pickup chance twice for husks: once in
        // `Zombie.finalizeSpawn` and again here.
        if spawn_reason != EntitySpawnReason::Conversion {
            self.set_can_pick_up_loot(
                rand::random::<f32>() < PICKUP_LOOT_CHANCE * difficulty_modifier,
            );
        }

        // TODO(camel_husk): vanilla wraps the group data in `HuskGroupData` and,
        // for natural spawns only, may mount a fresh CamelHusk with a Parched
        // rider. Both entities and natural spawning are out of scope for now.
        group_data
    }
});

impl_zombie_entity!(HuskEntity, {});
impl_zombie_living!(HuskEntity, {});
impl_zombie_mob!(HuskEntity, {});

impl Monster for HuskEntity {}

impl PathfinderMob for HuskEntity {}
