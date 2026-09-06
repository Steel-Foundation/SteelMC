use std::iter::empty;
use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::EndermiteEntityData;
use steel_registry::{sound_events, vanilla_attributes};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::ai::goal::{
    ClimbOnTopOfPowderSnowGoal, FloatGoal, HurtByTargetGoal, LookAtPlayerGoal, MeleeAttackGoal,
    NearestAttackableTargetGoal, RandomLookAroundGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySyncedData, LivingEntity,
    LivingEntityBase, Mob, MobBase, Monster, PathfinderMob, RemovalReason,
};
use crate::physics::MoveResult;
use crate::world::World;

/// Vanilla `Attributes.STEP_HEIGHT` default, used when the attribute is absent.
const DEFAULT_STEP_HEIGHT: f32 = 0.6;
/// Vanilla `Endermite.MAX_LIFE`: ticks until the mob discards itself when not
/// persistence-required.
const MAX_LIFETIME: i32 = 2400;
/// Vanilla `Endermite` constructor: `this.xpReward = 3`.
const XP_REWARD: i32 = 3;

/// A hostile endermite entity.
#[entity_behavior(class = "Endermite")]
pub struct EndermiteEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<EndermiteEntityData>,
    lifetime: SyncMutex<i32>,
    player_spawned: SyncMutex<bool>,
}

// SAFETY: The owner-scoped type key uniquely identifies EndermiteEntity.
unsafe impl DowncastType for EndermiteEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/endermite");
}

impl EndermiteEntity {
    /// Creates a new endermite entity instance.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Loads a saved endermite entity.
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
        let mut entity_data = EndermiteEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(1, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, ClimbOnTopOfPowderSnowGoal::new());
            goal_selector.add_goal(2, MeleeAttackGoal::new(1.0, false));
            goal_selector.add_goal(3, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(7, LookAtPlayerGoal::new(8.0));
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
            lifetime: SyncMutex::new(0),
            player_spawned: SyncMutex::new(false),
        }
    }

    /// Returns true if the endermite was spawned by a player.
    pub fn player_spawned(&self) -> bool {
        *self.player_spawned.lock()
    }

    /// Sets whether the endermite was spawned by a player.
    pub fn set_player_spawned(&self, player_spawned: bool) {
        *self.player_spawned.lock() = player_spawned;
    }

    /// Returns the endermite's lifetime in ticks.
    pub fn lifetime(&self) -> i32 {
        *self.lifetime.lock()
    }

    /// Sets the endermite's lifetime in ticks.
    pub fn set_lifetime(&self, lifetime: i32) {
        *self.lifetime.lock() = lifetime;
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
    }
}

impl Entity for EndermiteEntity {
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

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_ENDERMITE_STEP, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("Lifetime", *self.lifetime.lock());
        nbt.insert("PlayerSpawned", i8::from(*self.player_spawned.lock()));
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        *self.lifetime.lock() = nbt.int("Lifetime").unwrap_or(0);
        *self.player_spawned.lock() = nbt.byte("PlayerSpawned").is_some_and(|b| b != 0);
    }
}

impl LivingEntity for EndermiteEntity {
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

    fn sound_volume(&self) -> f32 {
        0.4
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMITE_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ENDERMITE_DEATH)
    }

    fn should_drop_experience(&self) -> bool {
        Monster::should_drop_experience_monster(self)
    }

    fn should_drop_loot(&self, world: &World) -> bool {
        Monster::should_drop_loot_monster(self, world)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = Monster::monster_ai_step(self);
        // Vanilla `Endermite.aiStep`: the lifetime grows only while the mob is
        // not persistence-required, and the discard check runs unconditionally.
        if self.level().is_some() {
            let discard = {
                let mut lifetime = self.lifetime.lock();
                if !self.is_persistence_required() {
                    *lifetime += 1;
                }
                *lifetime >= MAX_LIFETIME
            };
            if discard {
                self.set_removed(RemovalReason::Discarded);
            }
        }
        result
    }
}

impl Mob for EndermiteEntity {
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
        Some(&sound_events::ENTITY_ENDERMITE_AMBIENT)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl Monster for EndermiteEntity {}

impl PathfinderMob for EndermiteEntity {}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use simdnbt::borrow::read_compound;
    use simdnbt::owned::NbtCompound;
    use steel_registry::{init_vanilla_registry, vanilla_entities};
    use steel_utils::{ChunkPos, Downcast as _};

    use super::{EndermiteEntity, MAX_LIFETIME};
    use crate::entity::{Entity, LivingEntity, Mob, SharedEntity, next_entity_id};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn endermite_discards_after_max_lifetime_unless_persistence_required() {
        init_vanilla_registry();
        let world = fresh_test_world("endermite_lifetime");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        // Vanilla `Endermite.aiStep` grows the lifetime each tick and discards
        // the mob once it reaches 2400.
        let endermite: SharedEntity = Arc::new(EndermiteEntity::new(
            &vanilla_entities::ENDERMITE,
            next_entity_id(),
            DVec3::new(8.0, 65.0, 8.0),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&endermite))
            .expect("test endermite should attach to the loaded chunk");
        let endermite = endermite
            .downcast_ref::<EndermiteEntity>()
            .expect("endermite is concrete");
        // One tick below the cap stays alive; the next tick crosses it.
        endermite.set_lifetime(MAX_LIFETIME - 2);
        endermite.ai_step();
        assert!(
            !endermite.is_removed(),
            "lifetime below the cap keeps the mob alive"
        );
        endermite.ai_step();
        assert!(endermite.is_removed(), "reaching the cap discards the mob");

        // A persistence-required mob never grows its lifetime, mirroring the
        // vanilla guard around the increment.
        let persistent: SharedEntity = Arc::new(EndermiteEntity::new(
            &vanilla_entities::ENDERMITE,
            next_entity_id(),
            DVec3::new(9.0, 65.0, 8.0),
            Arc::downgrade(&world),
        ));
        world
            .try_add_entity(Arc::clone(&persistent))
            .expect("test endermite should attach to the loaded chunk");
        let persistent = persistent
            .downcast_ref::<EndermiteEntity>()
            .expect("endermite is concrete");
        persistent.set_persistence_required();
        persistent.set_lifetime(MAX_LIFETIME - 1);
        persistent.ai_step();
        assert!(
            !persistent.is_removed(),
            "persistence-required endermites must not expire"
        );
    }

    #[test]
    fn endermite_nbt_round_trip() {
        init_vanilla_registry();

        let endermite =
            EndermiteEntity::new(&vanilla_entities::ENDERMITE, 1, DVec3::ZERO, Weak::new());

        assert!(!endermite.player_spawned());
        assert_eq!(endermite.lifetime(), 0);

        endermite.set_player_spawned(true);
        endermite.set_lifetime(1234);
        assert!(endermite.player_spawned());
        assert_eq!(endermite.lifetime(), 1234);

        let mut nbt = NbtCompound::new();
        endermite.save_additional(&mut nbt);

        assert_eq!(nbt.int("Lifetime"), Some(1234));
        assert_eq!(nbt.byte("PlayerSpawned"), Some(1));

        let loaded =
            EndermiteEntity::new(&vanilla_entities::ENDERMITE, 2, DVec3::ZERO, Weak::new());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        loaded.load_additional((&borrowed).into());

        assert!(loaded.player_spawned());
        assert_eq!(loaded.lifetime(), 1234);
    }
}
