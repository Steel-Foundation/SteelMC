//! Vanilla Villager entity — ported from Pumpkin (foundation-first).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_data::VillagerData;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::VillagerEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::inventory::menu::kinds::merchant;
use crate::villager::offers_for_seed;
use crate::entity::ai::goal::{AvoidEntityGoal, FloatGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal, HurtByTargetGoal};
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase, PathfinderMob, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

#[entity_behavior(class = "Villager")]
pub struct VillagerEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<VillagerEntityData>,
    offers: Arc<SyncMutex<Vec<crate::villager::MerchantOffer>>>,
}

unsafe impl DowncastType for VillagerEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/villager");
}

fn is_hostile_entity(target: &dyn LivingEntity, _world: &World) -> bool {
    let key = &target.entity_type().key;
    *key == vanilla_entities::ZOMBIE.key
        || *key == vanilla_entities::ZOMBIE_VILLAGER.key
        || *key == vanilla_entities::ZOMBIFIED_PIGLIN.key
        || *key == vanilla_entities::HUSK.key
        || *key == vanilla_entities::DROWNED.key
        || *key == vanilla_entities::VINDICATOR.key
        || *key == vanilla_entities::EVOKER.key
        || *key == vanilla_entities::PILLAGER.key
        || *key == vanilla_entities::ILLUSIONER.key
        || *key == vanilla_entities::RAVAGER.key
        || *key == vanilla_entities::VEX.key
}

impl VillagerEntity {
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
        let mut entity_data = VillagerEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.5));
            goal_selector.add_goal(2, AvoidEntityGoal::with_selector(8.0, 0.6, 0.6, |target, world| is_hostile_entity(target, world)));
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(0.5));
            goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(7, RandomLookAroundGoal::new());
        }
        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new());
        }
        let offers = Arc::new(SyncMutex::new(offers_for_seed("farmer", 1, base.id() as u64)));
        Self { base, entity_type, living_base, mob_base,
            entity_data: SyncMutex::new(entity_data), offers }
    }
    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() { return; }
        let display = self.living_base.mob_effect_display_state();
        { let mut d=self.entity_data.lock(); let l=d.living_entity_mut(); l.effect_particles.set(display.particles); l.effect_ambience.set(display.ambient); }
        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data.set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }
}

impl Entity for VillagerEntity {
    fn base(&self) -> &EntityBase { &self.base }
    fn entity_type(&self) -> EntityTypeRef { self.entity_type }
    fn base_tick(&self) { Mob::base_tick_mob(self); }
    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions { let s=LivingEntity::get_scale(self); if self.entity_type.fixed { self.entity_type.dimensions } else { self.entity_type.dimensions.scale(s) } }
    fn synced_data(&self) -> Option<&dyn EntitySyncedData> { Some(&self.entity_data) }
    fn update_data_before_sync(&self) { self.update_dirty_mob_effect_entity_data(); }
    fn sound_source(&self) -> SoundSource { SoundSource::Neutral }
    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) { self.play_sound(&steel_registry::sound_events::ENTITY_VILLAGER_WORK_ARMORER, 0.15, 1.0); }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        let data = self.entity_data.lock();
        let vd = data.villager_data.get();
        nbt.insert("Profession", vd.profession);
        nbt.insert("Type", vd.villager_type);
        nbt.insert("Level", vd.level);
    }
    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        let mut data = self.entity_data.lock();
        let profession = nbt.int("Profession").unwrap_or(0);
        let villager_type = nbt.int("Type").unwrap_or(0);
        let level = nbt.int("Level").unwrap_or(1);
        let mut vd = data.villager_data.get().clone();
        vd.profession = profession;
        vd.villager_type = villager_type;
        vd.level = level;
        data.villager_data.set(vd);
    }
}

impl LivingEntity for VillagerEntity {
    fn living_base(&self) -> &LivingEntityBase { &self.living_base }
    fn get_health(&self) -> f32 { *self.entity_data.lock().living_entity().health.get() }
    fn set_health(&self, h: f32) { let m=self.get_max_health(); let c=h.clamp(0.0,m); self.entity_data.lock().living_entity_mut().health.set(c); }
    fn sound_volume(&self) -> f32 { 0.4 }
    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_VILLAGER_HURT) }
    fn death_sound(&self) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_VILLAGER_DEATH) }
    fn server_ai_step(&self) { Mob::mob_server_ai_step(self); }
    fn ai_step(&self) -> Option<MoveResult> { let r=self.default_ai_step();
        r }
}

impl Mob for VillagerEntity {
    fn mob_base(&self) -> &MobBase { &self.mob_base }
    fn tick_goal_selectors(&self) { PathfinderMob::tick_pathfinder_goal_selectors(self); }
    fn tick_path_navigation(&self) { PathfinderMob::tick_pathfinder_path_navigation(self); }
    fn ambient_sound(&self) -> Option<SoundEventRef> { Some(&steel_registry::sound_events::ENTITY_VILLAGER_AMBIENT) }
    fn finalize_spawn(&self, world: &Arc<World>, r: EntitySpawnReason, g: Option<SpawnGroupData>) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, r, g)
    }
    fn mob_flags(&self) -> i8 { *self.entity_data.lock().mob().mob_flags.get() }
    fn set_mob_flags(&self, f: i8) { self.entity_data.lock().mob_mut().mob_flags.set(f); }

    fn mob_interact(&self, player: &Player, _hand: InteractionHand) -> InteractionResult {
        // === RUNTIME DEBUG INSTRUMENTATION ===
        eprintln!("\n[VILLAGER-{}] === MOB_INTERACT ===", self.id());
        eprintln!("[VILLAGER-{}] Player: {}", self.id(), player.id());

        let villager_data = self.entity_data.lock().villager_data.get().clone();
        eprintln!("[VILLAGER-{}] VillagerData: profession={}, type={}, level={}",
            self.id(), villager_data.profession, villager_data.villager_type, villager_data.level);

        let offers = Arc::clone(&self.offers);
        let offers_count = offers.lock().len();
        eprintln!("[VILLAGER-{}] Offers count: {}", self.id(), offers_count);

        eprintln!("[VILLAGER-{}] Calling player.open_menu(\"Villager\", ...)", self.id());

        player.open_menu("Villager", move |context| {
            eprintln!("[VILLAGER-FACTORY] Menu factory called, container_id={}", context.container_id);
            merchant(Arc::clone(&context.player.inventory), context.container_id, offers)
        });

        eprintln!("[VILLAGER-{}] player.open_menu returned", self.id());
        eprintln!("[VILLAGER-{}] Returning InteractionResult::Success", self.id());
        eprintln!("[VILLAGER-{}] === END MOB_INTERACT ===\n", self.id());

        InteractionResult::Success
    }
}

impl PathfinderMob for VillagerEntity {}
