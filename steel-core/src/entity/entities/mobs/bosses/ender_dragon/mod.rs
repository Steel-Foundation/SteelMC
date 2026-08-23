//! Vanilla Ender Dragon boss — boss bar, crystal healing, phase-driven flight.
//! Foundation-first: core entity with synced phase + boss-bar hook; full 13-phase AI is TODO.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::EnderDragonEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase, PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::world::World;

#[entity_behavior(class = "EnderDragon")]
pub struct EnderDragonEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<EnderDragonEntityData>,
}

unsafe impl DowncastType for EnderDragonEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/ender_dragon");
}

impl EnderDragonEntity {
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
        let mut entity_data = EnderDragonEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
        }
    }
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }
        let d = self.living_base.mob_effect_display_state();
        {
            let mut ed = self.entity_data.lock();
            let l = ed.living_entity_mut();
            l.effect_particles.set(d.particles);
            l.effect_ambience.set(d.ambient);
        }
        self.entity_data.set_base_invisible_flag(d.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || d.glowing);
    }
}

impl Entity for EnderDragonEntity {
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
    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) {}
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("DragonPhase", *self.entity_data.lock().phase.get());
    }
    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        if let Some(v) = nbt.int("DragonPhase") {
            self.entity_data.lock().phase.set(v);
        }
    }
}

impl LivingEntity for EnderDragonEntity {
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
    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> {
        Some(&steel_registry::sound_events::ENTITY_ENDER_DRAGON_HURT)
    }
    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&steel_registry::sound_events::ENTITY_ENDER_DRAGON_DEATH)
    }
    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }
    fn ai_step(&self) -> Option<MoveResult> {
        self.default_ai_step()
    }
    // TODO: boss bar (ServerBossEvent), crystal healing, wing/phase animation, breath attack,
    // portal handling — full 13-phase AI (EnderDragonPhase.*) deferred to a follow-up pass.
}

impl Mob for EnderDragonEntity {
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
        Some(&steel_registry::sound_events::ENTITY_ENDER_DRAGON_AMBIENT)
    }
    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        r: EntitySpawnReason,
        g: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, r, g)
    }
    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }
    fn set_mob_flags(&self, f: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(f);
    }
}

impl PathfinderMob for EnderDragonEntity {}
