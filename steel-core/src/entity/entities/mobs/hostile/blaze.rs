use std::iter::empty;
use std::sync::Weak;

use glam::DVec3;
use rand::Rng;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::BlazeEntityData;
use steel_registry::{sound_events, vanilla_attributes};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use crate::entity::ai::goal::{
    BlazeAttackGoal, HurtByTargetGoal, LookAtPlayerGoal, MoveTowardsRestrictionGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::path::PathType;
use crate::entity::damage::DamageSource;
use crate::entity::projectile::triangle_random;
use crate::entity::{
    DefaultEntityPush, Entity, EntityBase, EntityBaseLoad, EntityMovementEmission, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, Monster,
    PathfinderMob, SharedEntity, SpawnRule, sync_dirty_mob_effects,
};
use crate::physics::MoveResult;
use crate::world::{LevelReader, World};

const DEFAULT_STEP_HEIGHT: f32 = 0.6;
const XP_REWARD: i32 = 10;
/// Vanilla `Blaze.allowedHeightOffset` initial value.
const ALLOWED_HEIGHT_OFFSET_INITIAL: f32 = 0.5;
/// Vanilla triangle mode for the random allowed height offset.
const ALLOWED_HEIGHT_OFFSET_MODE: f64 = 0.5;
const ALLOWED_HEIGHT_OFFSET_DEVIATION: f64 = 6.891;
const ALLOWED_HEIGHT_OFFSET_CHANGE_INTERVAL: i32 = 100;
/// Vanilla `0.3F` literals, widened the way Java widens floats inside the
/// double expression.
const RISE_IMPULSE_TARGET_Y: f64 = 0.3_f32 as f64;
const RISE_IMPULSE_SCALE: f64 = 0.3_f32 as f64;
/// Vanilla pathfinding maluses.
const WATER_MALUS: f32 = -1.0;
const LAVA_MALUS: f32 = 8.0;
const FIRE_MALUS: f32 = 0.0;
/// Blazes never choose a water-friendly stroll target.
const STROLL_WATER_PROBABILITY: f32 = 0.0;
/// Charged flag bit in the synced `flags` byte.
const CHARGED_FLAG: i8 = 1;

/// A blaze entity.
#[entity_behavior(class = "Blaze", spawn_rule)]
pub struct BlazeEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<BlazeEntityData>,
    allowed_height_offset: SyncMutex<f32>,
    next_height_offset_change_tick: SyncMutex<i32>,
}

// SAFETY: The owner-scoped type key uniquely identifies BlazeEntity.
unsafe impl DowncastType for BlazeEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/blaze");
}

impl BlazeEntity {
    /// Vanilla `SpawnPlacements` predicate: `Monster.checkAnyLightMonsterSpawnRules`,
    /// which only delegates to `Mob.checkMobSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = Self::check_blaze_spawn_rules;

    /// Checks the Blaze placement predicate for the spawner-only dispatcher.
    pub(crate) fn check_blaze_spawn_rules(
        entity_type: EntityTypeRef,
        level: &dyn LevelReader,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
        _random: &mut dyn Rng,
    ) -> bool {
        <Self as Mob>::check_mob_spawn_rules(entity_type, level, spawn_reason, pos)
    }

    /// Creates a new blaze entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved blaze entity.
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
        {
            let mut malus = mob_base.pathfinding_malus().lock();
            malus.set(PathType::Water, WATER_MALUS);
            malus.set(PathType::Lava, LAVA_MALUS);
            malus.set(PathType::FireInNeighbor, FIRE_MALUS);
            malus.set(PathType::Fire, FIRE_MALUS);
        }

        let mut entity_data = BlazeEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(4, BlazeAttackGoal::new());
            goal_selector.add_goal(5, MoveTowardsRestrictionGoal::new(1.0));
            goal_selector.add_goal(
                7,
                WaterAvoidingRandomStrollGoal::with_probability(1.0, STROLL_WATER_PROBABILITY),
            );
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());

            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new().set_alert_others(empty()));
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
            );
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            allowed_height_offset: SyncMutex::new(ALLOWED_HEIGHT_OFFSET_INITIAL),
            next_height_offset_change_tick: SyncMutex::new(0),
        }
    }

    /// Returns whether this blaze is charged and readying a fireball volley.
    pub fn is_charged(&self) -> bool {
        *self.entity_data.lock().flags.get() & CHARGED_FLAG != 0
    }

    /// Sets the charged flag in synced data.
    pub fn set_charged(&self, charged: bool) {
        let mut data = self.entity_data.lock();
        let flags = *data.flags.get();
        data.flags.set(if charged {
            flags | CHARGED_FLAG
        } else {
            flags & !CHARGED_FLAG
        });
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        sync_dirty_mob_effects(&self.living_base, &self.entity_data);
    }

    /// Rolls a new allowed height offset every vanilla interval.
    fn tick_height_offset(&self) {
        let mut next_change = self.next_height_offset_change_tick.lock();
        *next_change -= 1;
        if *next_change > 0 {
            return;
        }

        *next_change = ALLOWED_HEIGHT_OFFSET_CHANGE_INTERVAL;
        *self.allowed_height_offset.lock() =
            triangle_random(ALLOWED_HEIGHT_OFFSET_MODE, ALLOWED_HEIGHT_OFFSET_DEVIATION) as f32;
    }

    /// Floats upward while the target is above the allowed height offset.
    fn apply_rise_impulse(&self) {
        let Some(target) = self.target() else {
            return;
        };
        let Some(target_living) = target.as_living_entity() else {
            return;
        };
        if target.get_eye_y() <= self.get_eye_y() + f64::from(*self.allowed_height_offset.lock())
            || !Mob::can_attack(self, target_living)
        {
            return;
        }

        let velocity = self.velocity();
        let impulse = (RISE_IMPULSE_TARGET_Y - velocity.y) * RISE_IMPULSE_SCALE;
        self.set_velocity(velocity + DVec3::new(0.0, impulse, 0.0));
        self.mark_velocity_sync();
    }
}

impl DefaultEntityPush for BlazeEntity {}

impl Entity for BlazeEntity {
    fn is_hard_collision_relevant(&self) -> bool {
        false
    }

    fn as_default_entity_push(&self) -> Option<&dyn DefaultEntityPush> {
        Some(self)
    }

    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn movement_emission(&self) -> EntityMovementEmission {
        EntityMovementEmission::Events
    }

    fn base_tick(&self) {
        Mob::base_tick_mob(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.entity_type.fixed {
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

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(f64::from(DEFAULT_STEP_HEIGHT)) as f32
    }

    fn sound_source(&self) -> SoundSource {
        Monster::sound_source_monster(self)
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        Monster::fall_sounds_monster(self)
    }

    fn swim_sound(&self) -> SoundEventRef {
        Monster::swim_sound_monster(self)
    }

    fn swim_splash_sound(&self) -> SoundEventRef {
        Monster::swim_splash_sound_monster(self)
    }

    /// Vanilla `Blaze.isOnFire` reports the charged state instead of fire ticks.
    fn is_on_fire(&self) -> bool {
        self.is_charged()
    }

    /// Vanilla `Blaze.getLightLevelDependentMagicValue` is a constant `1.0`.
    fn get_light_level_dependent_magic_value(&self) -> f32 {
        1.0
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
    }
}

impl LivingEntity for BlazeEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
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
        Some(&sound_events::ENTITY_BLAZE_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BLAZE_DEATH)
    }

    fn should_drop_experience(&self) -> bool {
        Monster::should_drop_experience_monster(self)
    }

    fn should_drop_loot(&self, world: &World) -> bool {
        Monster::should_drop_loot_monster(self, world)
    }

    fn server_ai_step(&self, entity: &SharedEntity) {
        Mob::mob_server_ai_step(self, entity);
    }

    fn ai_step(&self, entity: &SharedEntity) -> Option<MoveResult> {
        let velocity = self.velocity();
        if !self.on_ground() && velocity.y < 0.0 {
            self.set_velocity(velocity * DVec3::new(1.0, 0.6, 1.0));
        }
        Monster::monster_ai_step(self, entity)
    }
}

impl Mob for BlazeEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self, entity: &SharedEntity) {
        PathfinderMob::tick_pathfinder_goal_selectors(self, entity);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_BLAZE_AMBIENT)
    }

    fn custom_server_ai_step(&self) {
        self.tick_height_offset();
        self.apply_rise_impulse();
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl Monster for BlazeEntity {}

impl PathfinderMob for BlazeEntity {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::entity::next_entity_id;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn water_damages_the_blaze_on_its_ai_step() {
        init_vanilla_registry();
        init_behaviors();
        let fixture = fresh_test_world("blaze_water_damage");
        let world = &fixture.world;
        let position = DVec3::new(8.5, 64.0, 8.5);
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));
        assert!(world.set_block(
            BlockPos::new(8, 64, 8),
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let blaze = Arc::new(BlazeEntity::new(
            &vanilla_entities::BLAZE,
            next_entity_id(),
            position,
            Arc::downgrade(world),
        ));
        let entity: SharedEntity = Arc::<BlazeEntity>::clone(&blaze);

        assert!(Entity::update_fluid_interaction(&*blaze));
        assert!(Entity::is_in_water(&*blaze));
        LivingEntity::ai_step(&*blaze, &entity);

        assert_eq!(blaze.get_health(), 19.0);
    }
}
