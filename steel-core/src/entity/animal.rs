//! Shared vanilla `Animal` state and hooks.

use std::sync::Arc;

use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Identifier, UuidExt};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ai::path::PathType;
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{
    AgeableMob, AgeableMobBase, ENTITIES, EntitySpawnReason, Mob, MobBase, SharedEntity,
    next_entity_id,
};
use crate::player::Player;
use crate::world::{LevelReader, World};

const PARENT_AGE_AFTER_BREEDING: i32 = 6000;
const IN_LOVE_TIME: i32 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AnimalState {
    in_love: i32,
    love_cause: Option<Uuid>,
}

impl AnimalState {
    const fn new() -> Self {
        Self {
            in_love: 0,
            love_cause: None,
        }
    }
}

/// Runtime fields shared by vanilla animals.
#[derive(Debug)]
pub struct AnimalBase {
    state: SyncMutex<AnimalState>,
}

impl AnimalBase {
    /// Creates default animal runtime state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: SyncMutex::new(AnimalState::new()),
        }
    }

    pub fn initialize_pathfinding_malus(mob_base: &MobBase) {
        let mut malus = mob_base.pathfinding_malus().lock();
        malus.set(PathType::FireInNeighbor, 16.0);
        malus.set(PathType::Fire, -1.0);
    }

    /// Returns vanilla `Animal.inLove`.
    #[must_use]
    pub fn in_love_time(&self) -> i32 {
        self.state.lock().in_love
    }

    /// Sets vanilla `Animal.inLove`.
    pub fn set_in_love_time(&self, in_love: i32) {
        self.state.lock().in_love = in_love;
    }

    /// Decrements vanilla `Animal.inLove` when it is active.
    pub fn tick_in_love_time(&self) {
        let mut state = self.state.lock();
        if state.in_love > 0 {
            state.in_love -= 1;
        }
    }

    /// Returns vanilla `Animal.loveCause` as a persisted UUID.
    #[must_use]
    pub fn love_cause_uuid(&self) -> Option<Uuid> {
        self.state.lock().love_cause
    }

    /// Sets vanilla `Animal.loveCause` as a persisted UUID.
    pub fn set_love_cause_uuid(&self, love_cause: Option<Uuid>) {
        self.state.lock().love_cause = love_cause;
    }
}

impl Default for AnimalBase {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla-shaped behavior shared by entities that extend `Animal`.
pub trait Animal: AgeableMob {
    /// Returns shared animal runtime state.
    fn animal_base(&self) -> &AnimalBase;

    /// Returns vanilla `Animal.inLove`.
    fn in_love_time(&self) -> i32 {
        self.animal_base().in_love_time()
    }

    /// Sets vanilla `Animal.inLove`.
    fn set_in_love_time(&self, in_love: i32) {
        self.animal_base().set_in_love_time(in_love);
    }

    /// Returns vanilla `Animal.loveCause` as a persisted UUID.
    fn love_cause_uuid(&self) -> Option<Uuid> {
        self.animal_base().love_cause_uuid()
    }

    /// Sets vanilla `Animal.loveCause` as a persisted UUID.
    fn set_love_cause_uuid(&self, love_cause: Option<Uuid>) {
        self.animal_base().set_love_cause_uuid(love_cause);
    }

    /// Returns vanilla `Animal.isInLove`.
    fn is_in_love(&self) -> bool {
        self.in_love_time() > 0
    }

    /// Returns vanilla `Animal.canFallInLove`.
    fn can_fall_in_love(&self) -> bool {
        self.in_love_time() <= 0
    }

    /// Sets vanilla love mode and records the player that caused it.
    fn set_in_love(&self, player: Option<&Player>) {
        self.set_in_love_time(IN_LOVE_TIME);
        if let Some(player) = player {
            self.set_love_cause_uuid(Some(player.gameprofile.id));
        }

        self.broadcast_entity_event(EntityStatus::InLoveHearts);
    }

    /// Resets vanilla love mode without clearing the stored love cause.
    fn reset_love(&self) {
        self.set_in_love_time(0);
    }

    /// Returns vanilla `Animal.canMate`.
    fn can_mate(&self, partner: &dyn Animal) -> bool {
        self.uuid() != partner.uuid()
            && self.entity_type() == partner.entity_type()
            && self.is_in_love()
            && partner.is_in_love()
    }

    /// Returns whether the stack is valid food for this animal.
    fn is_food(&self, _item_stack: &ItemStack) -> bool {
        false
    }

    /// Returns vanilla `Animal.getBaseExperienceReward`.
    fn base_experience_reward_animal(&self) -> i32 {
        1 + rand::random_range(0..3)
    }

    /// Returns vanilla `Animal.getAmbientSoundInterval`.
    fn ambient_sound_interval_animal(&self) -> i32 {
        120
    }

    /// Returns vanilla `Animal.getWalkTargetValue`.
    fn animal_walk_target_value(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };

        if world.get_block_state(pos.below()).get_block() == &vanilla_blocks::GRASS_BLOCK {
            10.0
        } else {
            world.pathfinding_cost_from_light_levels(pos)
        }
    }

    /// Returns vanilla `Animal.isBrightEnoughToSpawn`.
    fn is_bright_enough_to_spawn(level: &dyn LevelReader, pos: BlockPos) -> bool
    where
        Self: Sized,
    {
        level.raw_brightness(pos, 0) > 8
    }

    /// Returns vanilla `Animal.checkAnimalSpawnRules`.
    fn check_animal_spawn_rules(
        level: &dyn LevelReader,
        spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool
    where
        Self: Sized,
    {
        let bright_enough = spawn_reason.ignores_light_requirements()
            || Self::is_bright_enough_to_spawn(level, pos);
        level
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::ANIMALS_SPAWNABLE_ON)
            && bright_enough
    }

    /// Plays this animal's vanilla eating sound.
    fn play_eating_sound(&self) {}

    /// Handles vanilla `Animal.mobInteract`.
    fn mob_interact_animal(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };

        if !self.is_food(&item_stack) {
            return self.mob_interact_ageable(player, hand);
        }

        let age = self.get_age();
        if age == 0 && self.can_fall_in_love() {
            Mob::use_player_item(self, player, hand);
            self.set_in_love(Some(player));
            self.play_eating_sound();
            return InteractionResult::SuccessServer;
        }

        if self.can_age_up() {
            Mob::use_player_item(self, player, hand);
            self.age_up(
                AgeableMobBase::get_speed_up_seconds_when_feeding(-age),
                true,
            );
            self.play_eating_sound();
            return InteractionResult::Success;
        }

        self.mob_interact_ageable(player, hand)
    }

    /// Creates a same-type offspring using the registered entity factory.
    fn create_breed_offspring(&self, world: &Arc<World>) -> Option<SharedEntity> {
        ENTITIES.create(
            self.entity_type(),
            next_entity_id(),
            self.position(),
            Arc::downgrade(world),
        )
    }

    /// Returns this animal's breedable variant key when offspring inherit it.
    fn breed_variant_key(&self) -> Option<&Identifier> {
        None
    }

    /// Applies a breedable variant key to offspring that inherit one.
    fn set_breed_variant_key(&self, _key: &Identifier) -> bool {
        false
    }

    /// Applies entity-specific state to freshly created breeding offspring.
    fn initialize_breed_offspring(&self, _partner: &dyn Animal, _offspring: &dyn Animal) {}

    /// Creates this animal's vanilla breeding offspring.
    fn get_breed_offspring(
        &self,
        world: &Arc<World>,
        partner: &dyn Animal,
    ) -> Option<SharedEntity> {
        let offspring = self.create_breed_offspring(world)?;
        let Some(offspring_animal) = offspring.as_animal() else {
            log::error!(
                "breeding entity type {} created non-animal offspring",
                self.entity_type().key
            );
            return None;
        };

        self.initialize_breed_offspring(partner, offspring_animal);
        Some(offspring)
    }

    /// Creates, initializes, and inserts vanilla breeding offspring.
    fn spawn_child_from_breeding(&self, world: &Arc<World>, partner: &dyn Animal) {
        let Some(offspring) = self.get_breed_offspring(world, partner) else {
            return;
        };

        {
            let Some(offspring_animal) = offspring.as_animal() else {
                log::error!(
                    "breeding entity type {} created non-animal offspring",
                    self.entity_type().key
                );
                return;
            };
            offspring_animal.set_baby(true);
            if let Err(error) = offspring_animal.try_set_position(self.position()) {
                log::error!(
                    "failed to position breeding offspring {} at parent {}: {error}",
                    offspring.id(),
                    self.id()
                );
                return;
            }
            offspring_animal.set_rotation((0.0, 0.0));
            offspring_animal.set_old_position_to_current();

            self.finalize_spawn_child_from_breeding(world, partner, Some(offspring_animal));
        }

        if let Err(error) = world.try_add_entity(offspring) {
            log::error!(
                "failed to add breeding offspring for entity {} to world: {error}",
                self.id()
            );
        }
    }

    /// Applies vanilla breeding side effects after offspring creation.
    fn finalize_spawn_child_from_breeding(
        &self,
        world: &Arc<World>,
        partner: &dyn Animal,
        _offspring: Option<&dyn Animal>,
    ) {
        if self
            .love_cause_uuid()
            .or_else(|| partner.love_cause_uuid())
            .is_some()
        {
            // TODO: Award the animals-bred stat and advancement once those foundations exist.
        }

        self.set_age(PARENT_AGE_AFTER_BREEDING);
        partner.set_age(PARENT_AGE_AFTER_BREEDING);
        self.reset_love();
        partner.reset_love();
        self.broadcast_entity_event(EntityStatus::InLoveHearts);

        if world.get_game_rule(&MOB_DROPS).as_bool() == Some(true) {
            let xp = rand::random_range(0..7) + 1;
            ExperienceOrbEntity::award(world, self.position(), xp);
        }
    }

    /// Ticks vanilla animal love state.
    fn tick_animal_love(&self) {
        if self.get_age() != 0 {
            self.reset_love();
            return;
        }

        self.animal_base().tick_in_love_time();
        // TODO: Spawn in-love heart particles every 10 ticks once particle spawning exists.
    }

    /// Runs vanilla `Animal.customServerAiStep`.
    fn custom_server_ai_step_animal(&self) {
        if self.get_age() != 0 {
            self.reset_love();
        }
    }

    /// Returns vanilla animal far-away despawn behavior.
    fn remove_when_far_away_animal(&self, _dist_sqr: f64) -> bool {
        false
    }

    /// Saves vanilla animal fields.
    fn save_animal(&self, nbt: &mut NbtCompound) {
        nbt.insert("InLove", self.in_love_time());
        if let Some(love_cause) = self.love_cause_uuid() {
            nbt.insert(
                "LoveCause",
                NbtTag::IntArray(love_cause.to_int_array().to_vec()),
            );
        }
    }

    /// Loads vanilla animal fields.
    fn load_animal(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_in_love_time(nbt.int("InLove").unwrap_or(0));
        if let Some(love_cause) = nbt.int_array("LoveCause")
            && let Some(uuid) = Uuid::from_int_array(&love_cause)
        {
            self.set_love_cause_uuid(Some(uuid));
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};
    use steel_utils::BlockStateId;

    use super::*;
    use crate::entity::entities::PigEntity;

    struct SpawnRuleLevel {
        below_pos: BlockPos,
        below_state: BlockStateId,
        raw_brightness: u8,
    }

    impl LevelReader for SpawnRuleLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            if pos == self.below_pos {
                return self.below_state;
            }

            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            self.raw_brightness
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    fn spawn_rule_level(block_below: BlockStateId, raw_brightness: u8) -> SpawnRuleLevel {
        SpawnRuleLevel {
            below_pos: BlockPos::new(0, 63, 0),
            below_state: block_below,
            raw_brightness,
        }
    }

    #[test]
    fn animal_spawn_rules_require_spawnable_block_tag() {
        init_test_registry();
        let level = spawn_rule_level(vanilla_blocks::STONE.default_state(), 15);

        assert!(!<PigEntity as Animal>::check_animal_spawn_rules(
            &level,
            EntitySpawnReason::Natural,
            BlockPos::new(0, 64, 0)
        ));
    }

    #[test]
    fn animal_spawn_rules_require_raw_brightness_above_eight() {
        init_test_registry();
        let level = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 8);

        assert!(!<PigEntity as Animal>::check_animal_spawn_rules(
            &level,
            EntitySpawnReason::Natural,
            BlockPos::new(0, 64, 0)
        ));

        let level = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 9);

        assert!(<PigEntity as Animal>::check_animal_spawn_rules(
            &level,
            EntitySpawnReason::Natural,
            BlockPos::new(0, 64, 0)
        ));
    }

    #[test]
    fn animal_spawn_rules_trial_spawner_ignores_light() {
        init_test_registry();
        let level = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 0);

        assert!(<PigEntity as Animal>::check_animal_spawn_rules(
            &level,
            EntitySpawnReason::TrialSpawner,
            BlockPos::new(0, 64, 0)
        ));
    }
}
