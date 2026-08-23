//! Vanilla IronGolem entity — ported from Pumpkin (foundation-first).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::IronGolemEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{FloatGoal, LookAtPlayerGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal, MeleeAttackGoal, HurtByTargetGoal, NearestAttackableTargetGoal};
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, PathfinderMob, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

#[entity_behavior(class = "IronGolem")]
pub struct IronGolemEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<IronGolemEntityData>,
}

unsafe impl DowncastType for IronGolemEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/iron_golem");
}

impl IronGolemEntity {
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(EntityBase::new(id, position, entity_type.dimensions, world), entity_type)
    }
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(EntityBase::from_load(load, entity_type.dimensions), entity_type)
    }
    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mob_base = MobBase::new();
        let mut entity_data = IronGolemEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(1, MeleeAttackGoal::new(1.0, true));
            goal_selector.add_goal(2, WaterAvoidingRandomStrollGoal::new(0.6));
            goal_selector.add_goal(3, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(4, RandomLookAroundGoal::new());
        }
        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new().alert_same_type());
            target_selector.add_goal(2, NearestAttackableTargetGoal::new(&vanilla_entities::PLAYER, false));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::ZOMBIE, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::ZOMBIE_VILLAGER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::ZOMBIFIED_PIGLIN, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::SKELETON, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::WITHER_SKELETON, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::SILVERFISH, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::SPIDER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::CAVE_SPIDER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::SLIME, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::MAGMA_CUBE, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::ZOGLIN, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::HOGLIN, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::PILLAGER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::VINDICATOR, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::EVOKER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::RAVAGER, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::VEX, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::PHANTOM, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::DROWNED, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::HUSK, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::STRAY, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::BOGGED, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::BREEZE, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::WARDEN, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::CREAKING, true));
            target_selector.add_goal(3, NearestAttackableTargetGoal::new(&vanilla_entities::WITHER, true));
        }
        Self { base, entity_type, living_base, mob_base,
            entity_data: SyncMutex::new(entity_data) }
    }
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() { return; }
        let display = self.living_base.mob_effect_display_state();
        { let mut d=self.entity_data.lock(); let l=d.living_entity_mut(); l.effect_particles.set(display.particles); l.effect_ambience.set(display.ambient); }
        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data.set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }
}

impl Entity for IronGolemEntity {
    fn base(&self) -> &EntityBase { &self.base }
    fn entity_type(&self) -> EntityTypeRef { self.entity_type }
    fn base_tick(&self) { Mob::base_tick_mob(self); }
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions { let s=LivingEntity::get_scale(self); if self.entity_type.fixed { self.entity_type.dimensions } else { self.entity_type.dimensions.scale(s) } }
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> { Some(&self.entity_data) }
    fn update_data_before_sync(&self) { self.update_dirty_mob_effect_entity_data(); }
    fn sound_source(&self) -> SoundSource { SoundSource::Neutral }
    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) { self.play_sound(&steel_registry::sound_events::ENTITY_IRON_GOLEM_STEP, 0.15, 1.0); }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("PlayerCreated", self.is_player_created());
    }
    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        if let Some(created) = nbt.byte("PlayerCreated") {
            self.set_player_created(created != 0);
        }
    }
}

impl LivingEntity for IronGolemEntity {
    fn living_base(&self) -> &LivingEntityBase { &self.living_base }
    fn get_health(&self) -> f32 { *self.entity_data.lock().living_entity().health.get() }
    fn set_health(&self, h: f32) { let m=self.get_max_health(); let c=h.clamp(0.0,m); self.entity_data.lock().living_entity_mut().health.set(c); }
    fn sound_volume(&self) -> f32 { 0.4 }
    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_IRON_GOLEM_HURT) }
    fn death_sound(&self) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_IRON_GOLEM_DEATH) }
    fn server_ai_step(&self) { Mob::mob_server_ai_step(self); }
    fn ai_step(&self) -> Option<MoveResult> { let r=self.default_ai_step();
        r }
}

impl Mob for IronGolemEntity {
    fn mob_base(&self) -> &MobBase { &self.mob_base }
    fn tick_goal_selectors(&self) { PathfinderMob::tick_pathfinder_goal_selectors(self); }
    fn tick_path_navigation(&self) { PathfinderMob::tick_pathfinder_path_navigation(self); }
    fn ambient_sound(&self) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_IRON_GOLEM_REPAIR) }
    fn finalize_spawn(&self, world: &Arc<World>, r: EntitySpawnReason, g: Option<SpawnGroupData>) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, r, g)
    }
    fn mob_flags(&self) -> i8 { *self.entity_data.lock().mob().mob_flags.get() }
    fn set_mob_flags(&self, f: i8) { self.entity_data.lock().mob_mut().mob_flags.set(f); }
}

impl IronGolemEntity {
    #[must_use]
    pub fn is_player_created(&self) -> bool {
        (self.entity_data.lock().flags.get() & 0x01) != 0
    }

    pub fn set_player_created(&self, value: bool) {
        let mut data = self.entity_data.lock();
        let flags = data.flags.get();
        let new_flags = if value { flags | 0x01 } else { flags & !0x01 };
        data.flags.set(new_flags);
    }
}

impl PathfinderMob for IronGolemEntity {}