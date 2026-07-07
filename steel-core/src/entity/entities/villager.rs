//! Villager entity implementation.

use std::str::FromStr;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::{entity_behavior, entity_impl};
use steel_protocol::packets::game::{AttributeSnapshot, EquipmentSlotItem, SoundSource};
use steel_registry::entity_data::VillagerData;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::VillagerEntityData;
use steel_registry::{
    REGISTRY, RegistryEntry, RegistryExt, sound_events, vanilla_attributes, vanilla_particle_types,
};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;

use crate::behavior::InteractionResult;
use crate::entity::ai::brain::{
    Activity, Brain, LookAtTargetSink, MemoryModuleType, MoveToTargetSink,
    NearestLivingEntitiesSensor, RandomStroll, Schedule, SetEntityLookTarget,
    AcquireBed, SetWalkTargetFromHome, AcquireJobSite, AssignProfession,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason,
    EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, MobEffectSyncChange,
    PathfinderMob, SharedEntity, SpawnGroupData, Villager
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

const VILLAGER_BABY_SCALE: f32 = 0.5;

const VILLAGER_DEFAULT_SCHEDULE: Schedule = Schedule::new(&[
    (10, Activity::Idle),
    (2000, Activity::Work),
    (9000, Activity::Meet),
    (11000, Activity::Idle),
    (12000, Activity::Rest),
]);

#[entity_behavior(class = "Villager")]
pub struct VillagerEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    entity_data: SyncMutex<VillagerEntityData>,
    brain: SyncMutex<Brain>,
}

impl VillagerEntity {
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

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
        let ageable_base = AgeableMobBase::new();
        let mut entity_data = VillagerEntityData::new();

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            entity_data: SyncMutex::new(entity_data),
            brain: SyncMutex::new(Self::make_brain()),
        }
    }

    #[must_use]
    pub fn villager_data(&self) -> VillagerData {
        *self.entity_data.lock().villager_data.get()
    }

    pub fn set_villager_data(&self, data: VillagerData) {
        self.entity_data.lock().villager_data.set(data);
    }

    #[must_use]
    pub fn get_age(&self) -> i32 {
        AgeableMob::get_age(self)
    }

    pub fn set_age(&self, age: i32) {
        AgeableMob::set_age(self, age);
    }

    #[must_use]
    pub fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
    }

    pub fn set_baby(&self, baby: bool) {
        AgeableMob::set_baby(self, baby);
    }
}

#[entity_impl(class(ageable_mob))]
impl Entity for VillagerEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby() {
            self.entity_type
                .dimensions
                .scale(VILLAGER_BABY_SCALE * scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn tick(&self) {
        self.default_tick();
        self.living_base.decrement_invulnerable_time();
        self.tick_mob_effects();

        if self.is_dead_or_dying() {
            LivingEntity::tick_death(self);
            self.tick_living_state();
            return;
        }

        if !self.is_removed() {
            self.ai_step();
        }

        self.tick_living_state();
    }

    fn check_despawn(&self) {
        Mob::check_mob_despawn(self);
    }

    fn is_alive(&self) -> bool {
        !self.is_removed() && self.get_health() > 0.0
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        Entity::is_alive(self) && !self.is_spectator() && !self.on_climbable()
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        self.controlling_passenger_mob()
    }

    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement() && !self.is_no_ai()
    }

    fn get_default_gravity(&self) -> f64 {
        LivingEntity::get_attribute_gravity(self)
    }

    fn can_freeze(&self) -> bool {
        self.default_living_can_freeze()
    }

    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_living_can_walk_on_powder_snow()
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base.drain_dirty_mob_effects()
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.pack_living_equipment()
    }

    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.drain_dirty_living_equipment()
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(0.6) as f32
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        LivingEntity::hurt_server(self, source, amount)
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        Mob::interact_mob(self, player, hand, location)
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);

        let data = self.villager_data();
        let mut villager_data = NbtCompound::new();
        if let Some(villager_type) = usize::try_from(data.villager_type)
            .ok()
            .and_then(|id| REGISTRY.villager_types.by_id(id))
        {
            villager_data.insert("type", villager_type.key.to_string());
        }
        if let Some(profession) = usize::try_from(data.profession)
            .ok()
            .and_then(|id| REGISTRY.villager_professions.by_id(id))
        {
            villager_data.insert("profession", profession.key.to_string());
        }
        villager_data.insert("level", data.level);
        nbt.insert("VillagerData", NbtTag::Compound(villager_data));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);

        if let Some(villager_data) = nbt.compound("VillagerData") {
            let mut data = self.villager_data();
            if let Some(type_key) = villager_data.string("type")
                && let Ok(key) = Identifier::from_str(type_key.to_str().as_ref())
                && let Some(id) = REGISTRY.villager_types.id_from_key(&key)
            {
                data.villager_type = i32::try_from(id).unwrap_or(data.villager_type);
            }
            if let Some(profession_key) = villager_data.string("profession")
                && let Ok(key) = Identifier::from_str(profession_key.to_str().as_ref())
                && let Some(id) = REGISTRY.villager_professions.id_from_key(&key)
            {
                data.profession = i32::try_from(id).unwrap_or(data.profession);
            }
            if let Some(level) = villager_data.int("level") {
                data.level = level;
            }
            self.set_villager_data(data);
        }
    }
}

impl VillagerEntity {
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }

        let Some(particle_type_id) = vanilla_particle_types::ENTITY_EFFECT.try_id() else {
            log::error!("vanilla entity_effect particle type is not registered");
            return;
        };
        let Ok(particle_type_id) = i32::try_from(particle_type_id) else {
            log::error!("vanilla entity_effect particle type id does not fit protocol i32");
            return;
        };
        let display = self.living_base.mob_effect_display_state(particle_type_id);

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

    fn make_brain() -> Brain {
        let mut brain = Brain::new(
            [MemoryModuleType::LookTarget, MemoryModuleType::WalkTarget, MemoryModuleType::Home, MemoryModuleType::JobSite],
            vec![Box::new(NearestLivingEntitiesSensor)],
        );
        brain.set_core_activities([Activity::Core]);
        brain.add_activity(
            Activity::Core,
            0,
            vec![
                Box::new(MoveToTargetSink::new(150, 250)),
                Box::new(LookAtTargetSink::new(45, 90)),
                Box::new(AcquireBed::new(48)),
                Box::new(AcquireJobSite::new(48)),
                Box::new(AssignProfession::new()),
            ],
        );
        brain.add_activity(
            Activity::Idle,
            0,
            vec![
                Box::new(RandomStroll::new(0.5)),
                Box::new(SetEntityLookTarget::new(
                    |entity| entity.as_player().is_some(),
                    8.0,
                )),
            ],
        );
        brain.add_activity(Activity::Rest, 0, vec![Box::new(SetWalkTargetFromHome::new(0.6, 1))]);
        brain.add_activity(Activity::Work, 0, Vec::new());
        brain.add_activity(Activity::Meet, 0, Vec::new());
        brain.set_schedule(VILLAGER_DEFAULT_SCHEDULE);
        brain.use_default_activity();
        brain
    }
}

impl LivingEntity for VillagerEntity {
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

    fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();
        AgeableMob::tick_ageable_mob(self);
        result
    }
}

impl AgeableMob for VillagerEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }

    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }

    fn set_age_locked(&self, age_locked: bool) {
        self.entity_data
            .lock()
            .ageable_mob_mut()
            .age_locked
            .set(age_locked);
    }

    fn set_synced_baby(&self, baby: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(baby);
    }

    fn age_boundary_changed(&self, _baby: bool) {
        self.refresh_dimensions();
    }
}

impl Mob for VillagerEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn as_villager(&self) -> Option<&dyn Villager> {
        Some(self)
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_VILLAGER_AMBIENT)
    }

    fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
        false
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn custom_server_ai_step(&self) {
        let (game_time, day_time) = self
            .level()
            .map_or((0, 0), |world| (world.game_time(), world.day_time()));
        let mut brain = self.brain.lock();
        brain.update_activity_from_schedule(game_time, day_time);
        brain.tick(self, game_time);
    }
}

impl PathfinderMob for VillagerEntity {}

impl Villager for VillagerEntity {
    fn villager_data(&self) -> VillagerData {
        *self.entity_data.lock().villager_data.get()
    }

    fn set_villager_data(&self, data:VillagerData) {
        self.entity_data.lock().villager_data.set(data);
    }
}
