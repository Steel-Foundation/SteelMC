//! Zombie family entity implementations.
//!
//! Includes base Zombie, Husk, Drowned, and Zombie Villager variants.

mod drowned;
mod husk;
mod zombie_villager;

pub use drowned::DrownedEntity;
pub use husk::HuskEntity;
pub use zombie_villager::ZombieVillagerEntity;

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::ZombieEntityData;
use steel_registry::{sound_events, vanilla_entities};
use steel_utils::locks::SyncMutex;
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::ai::goal::{
    FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal, MoveTowardsRestrictionGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, LivingEntitySyncedData, Mob, MobBase, MoveResult,
    PathfinderMob, SpawnGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::world::World;

/// Vanilla zombie entity.
#[entity_behavior(class = "Zombie")]
pub struct ZombieEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<ZombieEntityData>,
    in_water_time: SyncMutex<i32>,
    conversion_time: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ZombieEntity`.
unsafe impl DowncastType for ZombieEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/zombie");
}

impl ZombieEntity {
    /// Creates a new zombie entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a zombie entity from saved base data.
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
        let mut entity_data = ZombieEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(2, MeleeAttackGoal::new(1.0, false));
            goal_selector.add_goal(3, MoveTowardsRestrictionGoal::new(1.0));
            goal_selector.add_goal(7, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
        }

        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new());
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new(&vanilla_entities::PLAYER, true),
            );
            target_selector.add_goal(
                3,
                NearestAttackableTargetGoal::new(&vanilla_entities::VILLAGER, false),
            );
            target_selector.add_goal(
                3,
                NearestAttackableTargetGoal::new(&vanilla_entities::IRON_GOLEM, true),
            );
            target_selector.add_goal(
                5,
                NearestAttackableTargetGoal::new(&vanilla_entities::TURTLE, true),
            );
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            in_water_time: SyncMutex::new(0),
            conversion_time: SyncMutex::new(-1),
        }
    }

    /// Returns whether this zombie is a baby (vanilla `Zombie.isBaby`).
    #[must_use]
    pub fn is_baby(&self) -> bool {
        *self.entity_data.lock().baby.get()
    }

    /// Sets the baby flag (vanilla `Zombie.setBaby`).
    pub fn set_baby(&self, baby: bool) {
        self.entity_data.lock().baby.set(baby);
    }

    /// Returns whether this zombie is converting to a Drowned
    /// (vanilla `Zombie.isUnderWaterConverting`).
    #[must_use]
    pub fn is_under_water_converting(&self) -> bool {
        *self.entity_data.lock().drowned_conversion.get()
    }

    fn start_under_water_conversion(&self, time: i32) {
        *self.conversion_time.lock() = time;
        self.entity_data.lock().drowned_conversion.set(true);
    }

    /// Vanilla `Zombie.isSunSensitive`.
    const fn is_sun_sensitive() -> bool {
        true
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let display = self.living_base.mob_effect_display_state();

        {
            let mut entity_data = self.entity_data.lock();
            let living = entity_data.living_entity_mut();
            living.effect_particles.set(display.particles);
            living.effect_ambience.set(display.ambient);
        }

        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    fn tick_zombie_specific(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if !Entity::is_alive(self) || self.is_no_ai() {
            return;
        }

        // Drowning conversion (vanilla `Zombie.tick`).
        if self.is_under_water_converting() {
            let mut conversion_time = self.conversion_time.lock();
            *conversion_time -= 1;
            if *conversion_time < 0 {
                // TODO: Convert to Drowned once that entity exists.
            }
        } else if self.is_eye_in_water() {
            let mut in_water_time = self.in_water_time.lock();
            *in_water_time += 1;
            if *in_water_time >= 600 {
                drop(in_water_time);
                self.start_under_water_conversion(300);
            }
        } else {
            *self.in_water_time.lock() = -1;
        }

        // Sun sensitivity (vanilla `Monster.aiStep`).
        if Self::is_sun_sensitive()
            && world.difficulty() != Difficulty::Peaceful
            && world.sky_darkening() < 4
        {
            let eye_pos = BlockPos::new(
                self.position().x.floor() as i32,
                self.get_eye_y().floor() as i32,
                self.position().z.floor() as i32,
            );
            if world.can_see_sky(eye_pos)
                && self
                    .living_base()
                    .equipment()
                    .lock()
                    .get_ref(EquipmentSlot::Head)
                    .is_empty()
            {
                self.set_remaining_fire_ticks(2);
            }
        }
    }
}

impl Entity for ZombieEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby() {
            // TODO: Use vanilla baby dimensions (0.49 x 0.98) once EntityDimensions
            // construction is verified.
            self.entity_type.dimensions.scale(scale * 0.5)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_ZOMBIE_STEP, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("IsBaby", self.is_baby());
        nbt.insert("InWaterTime", *self.in_water_time.lock());
        let conversion_time = *self.conversion_time.lock();
        nbt.insert(
            "DrownedConversionTime",
            if self.is_under_water_converting() {
                conversion_time
            } else {
                -1
            },
        );
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);

        if let Some(baby) = nbt.byte("IsBaby") {
            self.set_baby(baby != 0);
        }
        if let Some(in_water_time) = nbt.int("InWaterTime") {
            *self.in_water_time.lock() = in_water_time;
        }
        if let Some(conversion_time) = nbt.int("DrownedConversionTime") {
            if conversion_time == -1 {
                self.entity_data.lock().drowned_conversion.set(false);
            } else {
                self.start_under_water_conversion(conversion_time);
            }
        }
    }
}

impl LivingEntity for ZombieEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn living_synced_data(&self) -> Option<&dyn LivingEntitySyncedData> {
        Some(&self.entity_data)
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(0.0, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();
        self.tick_zombie_specific();
        result
    }
}

impl Mob for ZombieEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ZOMBIE_AMBIENT)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let group_data = self.finalize_spawn_mob_base(world, spawn_reason, group_data);

        // Vanilla `Zombie.finalizeSpawn` baby chance.
        if spawn_reason != EntitySpawnReason::Conversion {
            self.set_baby(rand::random::<f32>() < 0.05);
        }

        // Vanilla `Zombie.finalizeSpawn` can-pick-up-loot.
        if spawn_reason != EntitySpawnReason::Conversion {
            let difficulty = world.difficulty();
            let chance = match difficulty {
                Difficulty::Hard => 0.55,
                Difficulty::Normal => 0.55 * 0.75,
                Difficulty::Easy => 0.55 * 0.5,
                Difficulty::Peaceful => 0.0,
            };
            *self.mob_base().can_pick_up_loot().lock() = rand::random::<f32>() < chance as f32;
        }

        group_data
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for ZombieEntity {}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_entities};

    use super::*;
    use crate::entity::Entity;

    #[test]
    fn zombie_creates_with_correct_entity_type() {
        init_vanilla_registry();
        let zombie = ZombieEntity::new(&vanilla_entities::ZOMBIE, 1, DVec3::ZERO, Weak::new());
        assert_eq!(zombie.entity_type(), &vanilla_entities::ZOMBIE);
    }

    #[test]
    fn zombie_starts_as_non_baby() {
        init_vanilla_registry();
        let zombie = ZombieEntity::new(&vanilla_entities::ZOMBIE, 1, DVec3::ZERO, Weak::new());
        assert!(!zombie.is_baby());
    }

    #[test]
    fn zombie_set_baby_updates_synced_data() {
        init_vanilla_registry();
        let zombie = ZombieEntity::new(&vanilla_entities::ZOMBIE, 1, DVec3::ZERO, Weak::new());
        zombie.set_baby(true);
        assert!(zombie.is_baby());
    }

    #[test]
    fn zombie_has_goals_registered() {
        init_vanilla_registry();
        let zombie = ZombieEntity::new(&vanilla_entities::ZOMBIE, 1, DVec3::ZERO, Weak::new());
        let goal_selector = zombie.mob_base().goal_selector().lock();
        let target_selector = zombie.mob_base().target_selector().lock();
        // 6 goal-selector goals + 5 target-selector goals.
        assert!(goal_selector.available_goal_count() >= 6);
        assert!(target_selector.available_goal_count() >= 5);
    }
}
