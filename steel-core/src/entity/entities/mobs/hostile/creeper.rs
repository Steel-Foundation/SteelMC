// Adapted from Pumpkin (GPL-3.0): https://github.com/Snowiiii/Pumpkin
//! Creeper entity implementation

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::CreeperEntityData;
use steel_registry::{
    REGISTRY, TaggedRegistryExt as _, sound_events, vanilla_entities, vanilla_game_events,
    vanilla_item_tags::ItemTag, vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey};

use std::sync::Weak;

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    AvoidEntityGoal, FloatGoal, Goal, GoalControls, HurtByTargetGoal, LookAtPlayerGoal,
    MeleeAttackGoal, NearestAttackableTargetGoal, RandomLookAroundGoal,
    WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySpawnReason, EntitySyncedData,
    LivingEntity, LivingEntityBase, LivingEntitySyncedData, Mob, MobBase, PathfinderMob,
    RemovalReason, SharedEntity, SpawnGroupData,
};
use crate::player::Player;
use crate::world::World;
use crate::world::explosion::ExplosionInteraction;

/// Vanilla creeper entity.
#[entity_behavior(class = "Creeper")]
pub struct CreeperEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    entity_data: SyncMutex<CreeperEntityData>,
    /// Vanilla `Creeper.swell`: fuse progress in ticks.
    swell: SyncMutex<i32>,
    /// Vanilla `Creeper.oldSwell`, used for client-side fuse interpolation.
    old_swell: SyncMutex<i32>,
    /// Vanilla `Creeper.maxSwell`, persisted as the `Fuse` NBT tag.
    max_swell: SyncMutex<i32>,
    /// Vanilla `Creeper.explosionRadius`, persisted as `ExplosionRadius`.
    explosion_radius: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `CreeperEntity`.
unsafe impl DowncastType for CreeperEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/creeper");
}

/// Vanilla `SwellGoal`: stops movement and drives the synced swell direction while a
/// target stays within fuse range with line of sight.
struct SwellGoal {
    target: Option<SharedEntity>,
}

/// Vanilla `SwellGoal`'s `distanceToSqr(target) < 9.0` can-use range.
const SWELL_START_RANGE_SQR: f64 = 9.0;
/// Vanilla `SwellGoal`'s `distanceToSqr(target) > 49.0` cancel range.
const SWELL_CANCEL_RANGE_SQR: f64 = 49.0;

impl SwellGoal {
    const fn new() -> Self {
        Self { target: None }
    }

    fn creeper(mob: &dyn PathfinderMob) -> Option<&CreeperEntity> {
        mob.downcast_ref::<CreeperEntity>()
    }
}

impl Goal for SwellGoal {
    fn controls(&self) -> GoalControls {
        // Claiming MOVE overrides the melee attack goal so the creeper stands still
        // while its fuse burns, matching vanilla.
        GoalControls::MOVE
    }

    fn can_use(&mut self, mob: &dyn PathfinderMob) -> bool {
        let Some(creeper) = Self::creeper(mob) else {
            return false;
        };
        if creeper.swell_dir() > 0 {
            return true;
        }
        let Some(target) = mob.target() else {
            return false;
        };
        target.is_alive()
            && mob.position().distance_squared(target.position()) < SWELL_START_RANGE_SQR
    }

    fn start(&mut self, mob: &dyn PathfinderMob) {
        mob.mob_base().navigation().lock().stop();
        self.target = mob.target();
    }

    fn stop(&mut self, _mob: &dyn PathfinderMob) {
        self.target = None;
    }

    fn requires_update_every_tick(&self) -> bool {
        true
    }

    fn tick(&mut self, mob: &dyn PathfinderMob) {
        let Some(creeper) = Self::creeper(mob) else {
            return;
        };
        let Some(target) = self.target.as_ref() else {
            creeper.set_swell_dir(-1);
            return;
        };
        if !target.is_alive()
            || mob.position().distance_squared(target.position()) > SWELL_CANCEL_RANGE_SQR
            || !mob.has_line_of_sight_cached(target.as_ref())
        {
            creeper.set_swell_dir(-1);
        } else {
            creeper.set_swell_dir(1);
        }
    }
}

impl CreeperEntity {
    /// Vanilla `Creeper.DEFAULT_MAX_SWELL`.
    const DEFAULT_MAX_SWELL: i32 = 30;
    /// Vanilla `Creeper.DEFAULT_EXPLOSION_RADIUS`.
    const DEFAULT_EXPLOSION_RADIUS: i32 = 3;

    /// Creates a new creeper entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a creeper entity from saved base data.
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
        let mut entity_data = CreeperEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(1, FloatGoal::new(&mob_base));
            goal_selector.add_goal(2, SwellGoal::new());
            goal_selector.add_goal(
                3,
                AvoidEntityGoal::with_selector(6.0, 1.0, 1.2, |target, _| {
                    let entity_type = target.entity_type();
                    entity_type == &vanilla_entities::OCELOT
                        || entity_type == &vanilla_entities::CAT
                }),
            );
            goal_selector.add_goal(4, MeleeAttackGoal::new(1.0, false));
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(0.8));
            goal_selector.add_goal(6, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(6, RandomLookAroundGoal::new());
        }

        {
            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(
                1,
                NearestAttackableTargetGoal::new_for_players(true, |_, _| true),
            );
            target_selector.add_goal(2, HurtByTargetGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            entity_data: SyncMutex::new(entity_data),
            swell: SyncMutex::new(0),
            old_swell: SyncMutex::new(0),
            max_swell: SyncMutex::new(Self::DEFAULT_MAX_SWELL),
            explosion_radius: SyncMutex::new(Self::DEFAULT_EXPLOSION_RADIUS),
        }
    }

    /// Returns whether this creeper is charged (powered).
    #[must_use]
    pub fn is_charged(&self) -> bool {
        *self.entity_data.lock().is_powered.get()
    }

    /// Sets the powered (charged) flag.
    pub fn set_charged(&self, charged: bool) {
        self.entity_data.lock().is_powered.set(charged);
    }

    /// Returns vanilla `Creeper.isIgnited` from synced data.
    #[must_use]
    pub fn is_ignited(&self) -> bool {
        *self.entity_data.lock().is_ignited.get()
    }

    /// Vanilla `Creeper.ignite`.
    pub fn ignite(&self) {
        self.entity_data.lock().is_ignited.set(true);
    }

    /// Vanilla `Creeper.getSwellDir` from synced data so clients render the fuse.
    #[must_use]
    pub fn swell_dir(&self) -> i32 {
        *self.entity_data.lock().swell_dir.get()
    }

    /// Vanilla `Creeper.setSwellDir`.
    pub fn set_swell_dir(&self, dir: i32) {
        self.entity_data.lock().swell_dir.set(dir);
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

    /// Runs vanilla `Creeper.tick`'s fuse accounting; called from `base_tick` before the
    /// mob tick, mirroring vanilla's position ahead of `super.tick()`.
    fn tick_creeper_specific(&self) {
        if !Entity::is_alive(self) {
            return;
        }

        *self.old_swell.lock() = *self.swell.lock();

        if self.is_ignited() {
            self.set_swell_dir(1);
        }

        let swell_dir = self.swell_dir();
        if swell_dir > 0 && *self.swell.lock() == 0 {
            self.play_sound(&sound_events::ENTITY_CREEPER_PRIMED, 1.0, 0.5);
            self.game_event(&vanilla_game_events::PRIME_FUSE);
        }

        {
            let mut swell = self.swell.lock();
            *swell += swell_dir;
            if *swell < 0 {
                *swell = 0;
            }
        }

        let max_swell = *self.max_swell.lock();
        if *self.swell.lock() >= max_swell {
            *self.swell.lock() = max_swell;
            self.explode_creeper();
        }
    }

    /// Vanilla `Creeper.explodeCreeper`.
    fn explode_creeper(&self) {
        let Some(world) = self.level() else {
            return;
        };

        let explosion_multiplier = if self.is_charged() { 2.0 } else { 1.0 };
        let radius = f32::from(*self.explosion_radius.lock() as u8) * explosion_multiplier;
        // Marked removed before exploding so this path cannot run twice and the mob
        // stops ticking as if alive (vanilla sets `dead` for the same reason).
        self.set_removed(RemovalReason::Killed);
        world.explode(
            Some(self as &dyn Entity),
            self.position(),
            radius,
            false,
            ExplosionInteraction::Mob,
        );
        // TODO: Vanilla spawns an AreaEffectCloud when a creeper with active mob
        // effects explodes; Steel has no AreaEffectCloud entity yet.
    }
}

impl Entity for CreeperEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        self.tick_creeper_specific();
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

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(&sound_events::ENTITY_CREEPER_PRIMED, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        nbt.insert("powered", self.is_charged());
        nbt.insert("Fuse", *self.max_swell.lock() as i16);
        nbt.insert("ExplosionRadius", *self.explosion_radius.lock() as i8);
        nbt.insert("ignited", self.is_ignited());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);

        if let Some(powered) = nbt.byte("powered") {
            self.set_charged(powered != 0);
        }
        if let Some(fuse) = nbt.short("Fuse") {
            *self.max_swell.lock() = i32::from(fuse);
        }
        if let Some(radius) = nbt.byte("ExplosionRadius") {
            *self.explosion_radius.lock() = i32::from(radius);
        }
        if nbt.byte("ignited").is_some_and(|ignited| ignited != 0) {
            self.ignite();
        }
    }
}

impl LivingEntity for CreeperEntity {
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
        Some(&sound_events::ENTITY_CREEPER_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_CREEPER_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }
}

impl Mob for CreeperEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn finalize_spawn(
        &self,
        world: &std::sync::Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_mob_base(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    /// Vanilla `Creeper.mobInteract`: flint and steel / fire charge ignite the fuse.
    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let held_item = {
            let inventory = player.inventory.lock();
            let stack = inventory.get_item_in_hand(hand);
            stack.copy_with_count(stack.count())
        };
        if !REGISTRY
            .items
            .is_in_tag(held_item.item(), &ItemTag::CREEPER_IGNITERS)
        {
            return InteractionResult::Pass;
        }

        let sound = if held_item.is(&vanilla_items::FIRE_CHARGE) {
            &sound_events::ITEM_FIRECHARGE_USE
        } else {
            &sound_events::ITEM_FLINTANDSTEEL_USE
        };
        let pitch = rand::random::<f32>() * 0.4 + 0.8;
        self.play_sound(sound, 1.0, pitch);
        self.ignite();

        // Vanilla shrinks non-damageable igniters and hurts damageable ones.
        if held_item.is_damageable_item() {
            player
                .inventory
                .lock()
                .hurt_item_in_hand(hand, 1, player.has_infinite_materials());
        } else {
            player.inventory.lock().shrink_item_in_hand(hand, 1);
        }

        InteractionResult::SuccessServer
    }
}

impl PathfinderMob for CreeperEntity {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::ChunkPos;

    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use super::*;

    fn test_creeper(world: &Arc<World>) -> CreeperEntity {
        CreeperEntity::new(
            &vanilla_entities::CREEPER,
            1,
            DVec3::new(0.5, 64.0, 0.5),
            Arc::downgrade(world),
        )
    }

    #[test]
    fn ignited_fuse_reaches_max_and_removes_creeper() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("creeper_ignited_fuse_explodes");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        // A block at the creeper's own position is inside every explosion ray.
        let blast_pos = BlockPos::new(0, 64, 0);
        assert!(world.set_block(
            blast_pos,
            vanilla_blocks::STONE.default_state(),
            steel_utils::types::UpdateFlags::UPDATE_ALL,
        ));

        let creeper = test_creeper(&world);
        creeper.ignite();
        assert!(creeper.is_ignited());

        // The first tick priming the fuse sets the synced swell direction to 1,
        // which is what drives the client's fuse animation.
        creeper.base_tick();
        assert_eq!(creeper.swell_dir(), 1);

        for _ in 0..CreeperEntity::DEFAULT_MAX_SWELL as u32 + 5 {
            creeper.base_tick();
            if creeper.is_removed() {
                break;
            }
        }

        assert_eq!(creeper.removal_reason(), Some(RemovalReason::Killed));
        assert!(world.get_block_state(blast_pos).is_air());
    }

    #[test]
    fn fuse_cannot_explode_twice_after_removal() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("creeper_no_double_explosion");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));

        let creeper = test_creeper(&world);
        creeper.ignite();
        for _ in 0..CreeperEntity::DEFAULT_MAX_SWELL as u32 + 5 {
            creeper.base_tick();
            if creeper.is_removed() {
                break;
            }
        }
        assert!(creeper.is_removed());

        // Ticking a removed creeper must be a no-op: the alive guard prevents the
        // explosion path from running again.
        for _ in 0..10 {
            creeper.base_tick();
        }
        assert_eq!(creeper.removal_reason(), Some(RemovalReason::Killed));
    }

    #[test]
    fn negative_swell_dir_decay_clamps_at_zero() {
        init_vanilla_registry();
        let world = fresh_test_world("creeper_swell_decay");
        let creeper = test_creeper(&world);

        creeper.set_swell_dir(1);
        creeper.tick_creeper_specific();
        assert_eq!(*creeper.swell.lock(), 1);

        creeper.set_swell_dir(-1);
        for _ in 0..5 {
            creeper.tick_creeper_specific();
        }
        assert_eq!(*creeper.swell.lock(), 0);
    }
}
