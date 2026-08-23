//! Vanilla Armadillo entity — ported from Pumpkin (foundation-first).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::ArmadilloEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

const DEFAULT_STEP_HEIGHT: f32 = 0.6;

#[entity_behavior(class = "Armadillo")]
pub struct ArmadilloEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<ArmadilloEntityData>,
}

unsafe impl DowncastType for ArmadilloEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/armadillo");
}

impl ArmadilloEntity {
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
        let animal_base = AnimalBase::new();
        AnimalBase::initialize_pathfinding_malus(&mob_base);
        let mut entity_data = ArmadilloEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.4));
            goal_selector.add_goal(2, BreedGoal::new(1.0));
            goal_selector.add_goal(4, FollowParentGoal::new(1.1));
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(7, RandomLookAroundGoal::new());
        }
        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
        }
    }
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }
        let display = self.living_base.mob_effect_display_state();
        {
            let mut d = self.entity_data.lock();
            let l = d.living_entity_mut();
            l.effect_particles.set(display.particles);
            l.effect_ambience.set(display.ambient);
        }
        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }
}

impl Entity for ArmadilloEntity {
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
        let s = LivingEntity::get_scale(self);
        if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(s)
        }
    }
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }
    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }
    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }
    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) {
        self.play_sound(
            &steel_registry::sound_events::ENTITY_ARMADILLO_STEP,
            0.15,
            1.0,
        );
    }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
    }
    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);
    }
}

impl LivingEntity for ArmadilloEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }
    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }
    fn set_health(&self, h: f32) {
        let m = self.get_max_health();
        let c = h.clamp(0.0, m);
        self.entity_data.lock().living_entity_mut().health.set(c);
    }
    fn sound_volume(&self) -> f32 {
        0.4
    }
    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> {
        Some(&steel_registry::sound_events::ENTITY_ARMADILLO_HURT)
    }
    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&steel_registry::sound_events::ENTITY_ARMADILLO_DEATH)
    }
    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }
    fn ai_step(&self) -> Option<MoveResult> {
        let r = self.default_ai_step();
        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        r
    }
}

impl AgeableMob for ArmadilloEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }
    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }
    fn set_age_locked(&self, v: bool) {
        self.entity_data.lock().ageable_mob_mut().age_locked.set(v);
    }
    fn set_synced_baby(&self, b: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(b);
    }
    fn age_boundary_changed(&self, _baby: bool) {
        self.refresh_dimensions();
    }
}

impl Animal for ArmadilloEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }
    fn is_food(&self, _s: &steel_registry::item_stack::ItemStack) -> bool {
        false
    }
}

impl Mob for ArmadilloEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }
    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }
    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }
    fn custom_server_ai_step(&self) {
        Animal::custom_server_ai_step_animal(self);
    }
    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(&steel_registry::sound_events::ENTITY_ARMADILLO_AMBIENT)
    }
    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        r: EntitySpawnReason,
        g: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_ageable_mob(world, r, g)
    }
    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }
    fn set_mob_flags(&self, f: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(f);
    }
    fn mob_interact(&self, p: &Player, h: InteractionHand) -> InteractionResult {
        Animal::mob_interact_animal(self, p, h)
    }
}

impl PathfinderMob for ArmadilloEntity {}
