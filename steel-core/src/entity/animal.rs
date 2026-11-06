//! Vanilla `Animal`: feeding, love mode and breeding.
//!
//! State and per-type properties live in the [`Breeding`] component;
//! behavior lives on the [`Animal`] view. The `aiStep`, `customServerAiStep`
//! and `mobInteract` layers are pipeline stages in this module, and
//! [`ai_step`] / [`mob_interact`] build an animal's whole vanilla chain.

use std::ops::RangeInclusive;
use std::sync::Arc;

use rand::Rng;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_rules::MOB_DROPS;
use steel_registry::{vanilla_blocks, vanilla_entities};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey, UuidExt};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ageable::{self, Age, Ageable};
use crate::entity::ai::path::PathType;
use crate::entity::component::definition::{MobDefinition, MobEntries};
use crate::entity::component::entries::{AiStep, CustomServerAiStep, MobInteract, MobInteractCtx};
use crate::entity::component::pipeline::{Flow, Pipeline, Stage, StageId};
use crate::entity::component::{Component, Components as _, Has};
use crate::entity::entities::ExperienceOrbEntity;
use crate::entity::{
    Entity, EntitySpawnReason, LivingProps, Mob, MobBase, MobProps, SharedEntity, mob,
    next_entity_id,
};
use crate::player::Player;
use crate::world::{LevelReader, World};

const PARENT_AGE_AFTER_BREEDING: i32 = 6000;
const IN_LOVE_TIME: i32 = 600;
const BREEDING_XP: RangeInclusive<i32> = 1..=7;
/// `Animal`'s changes to the living defaults.
#[must_use]
pub const fn living_props<E: Parts>() -> LivingProps<E> {
    LivingProps {
        base_experience_reward: |_| Animal::base_experience_reward(),
        ..mob::living_props()
    }
}

/// `Animal`'s changes to the mob defaults, for a type with `definition`.
#[must_use]
pub const fn mob_props(definition: fn() -> &'static dyn MobEntries) -> MobProps {
    MobProps {
        ambient_sound_interval: 120,
        remove_when_far_away: |_, _| false,
        pathfinder: true,
        walk_target_value: |mob, pos| walk_target_value(mob.entity(), pos),
        ..MobProps::new(definition)
    }
}

/// Per-type properties of a vanilla `Animal`.
pub struct BreedingProps {
    /// `Animal.isFood`.
    pub is_food: fn(&ItemStack) -> bool,
    /// `Animal.playEatingSound`; vanilla's default plays nothing.
    pub play_eating_sound: fn(&dyn Entity),
}

impl BreedingProps {
    /// Properties for an animal eating `is_food`, with vanilla's defaults
    /// for everything else.
    #[must_use]
    pub const fn new(is_food: fn(&ItemStack) -> bool) -> Self {
        Self {
            is_food,
            play_eating_sound: |_| {},
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LoveState {
    in_love: i32,
    love_cause: Option<Uuid>,
}

/// Love state and properties of a vanilla `Animal`.
pub struct Breeding {
    state: CheckedMutex<LoveState>,
    props: &'static BreedingProps,
}

impl Breeding {
    /// Not in love, with `props` describing this animal type.
    #[must_use]
    pub const fn new(props: &'static BreedingProps) -> Self {
        Self {
            state: CheckedMutex::new(
                "Breeding",
                LoveState {
                    in_love: 0,
                    love_cause: None,
                },
            ),
            props,
        }
    }

    /// The pathfinding malus every animal starts with (`Animal` constructor).
    pub fn initialize_pathfinding_malus(mob_base: &MobBase) {
        let mut malus = mob_base.pathfinding_malus().lock();
        malus.set(PathType::FireInNeighbor, 16.0);
        malus.set(PathType::Fire, -1.0);
    }
}

// SAFETY: This key is owned by Steel and uniquely identifies `Breeding`.
unsafe impl DowncastType for Breeding {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity_component/breeding");
}

impl Component for Breeding {
    fn save(&self, _owner: &dyn Entity, nbt: &mut NbtCompound) {
        let state = *self.state.lock();
        nbt.insert("InLove", state.in_love);
        if let Some(love_cause) = state.love_cause {
            nbt.insert(
                "LoveCause",
                NbtTag::IntArray(love_cause.to_int_array().to_vec()),
            );
        }
    }

    fn load(&self, _owner: &dyn Entity, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let mut state = self.state.lock();
        state.in_love = nbt.int("InLove").unwrap_or(0);
        if let Some(love_cause) = nbt.int_array("LoveCause")
            && let Some(uuid) = Uuid::from_int_array(&love_cause)
        {
            state.love_cause = Some(uuid);
        }
    }
}

/// An entity seen as a vanilla `Animal`.
#[derive(Clone, Copy)]
pub struct Animal<'a> {
    ageable: Ageable<'a>,
    breeding: &'a Breeding,
}

impl<'a> Animal<'a> {
    /// Pairs `entity` with its animal components.
    #[must_use]
    pub const fn new(entity: &'a dyn Entity, age: &'a Age, breeding: &'a Breeding) -> Self {
        Self {
            ageable: Ageable::new(entity, age),
            breeding,
        }
    }

    /// Returns `entity` as an animal, if it is one.
    #[must_use]
    pub fn of(entity: &'a dyn Entity) -> Option<Self> {
        Some(Self::new(
            entity,
            entity.query::<Age>()?,
            entity.query::<Breeding>()?,
        ))
    }

    /// Returns this animal's age view.
    #[must_use]
    pub const fn ageable(&self) -> Ageable<'a> {
        self.ageable
    }

    /// Returns the entity this view belongs to.
    #[must_use]
    pub const fn entity(&self) -> &'a dyn Entity {
        self.ageable.entity()
    }

    /// Returns whether `item_stack` is this animal's food (`Animal.isFood`).
    #[must_use]
    pub fn is_food(&self, item_stack: &ItemStack) -> bool {
        (self.breeding.props.is_food)(item_stack)
    }

    /// Ticks remaining in love mode.
    #[must_use]
    pub fn in_love_time(&self) -> i32 {
        self.breeding.state.lock().in_love
    }

    /// Returns whether this animal is in love mode.
    #[must_use]
    pub fn is_in_love(&self) -> bool {
        self.in_love_time() > 0
    }

    /// Returns whether this animal can be fed into love mode.
    #[must_use]
    pub fn can_fall_in_love(&self) -> bool {
        self.in_love_time() <= 0
    }

    /// The player who caused this animal to fall in love, if any.
    #[must_use]
    pub fn love_cause(&self) -> Option<Uuid> {
        self.breeding.state.lock().love_cause
    }

    /// Starts love mode, recording `player` as its cause (`Animal.setInLove`).
    pub fn set_in_love(&self, player: Option<&Player>) {
        {
            let mut state = self.breeding.state.lock();
            state.in_love = IN_LOVE_TIME;
            if let Some(player) = player {
                state.love_cause = Some(player.gameprofile.id());
            }
        }
        self.entity()
            .broadcast_entity_event(EntityStatus::InLoveHearts);
    }

    /// Ends love mode without clearing its cause (`Animal.resetLove`).
    pub fn reset_love(&self) {
        self.breeding.state.lock().in_love = 0;
    }

    /// Returns whether this animal and `partner` can breed: distinct animals
    /// of the same type, both in love (`Animal.canMate`).
    #[must_use]
    pub fn can_mate(&self, partner: &Animal<'_>) -> bool {
        self.entity().uuid() != partner.entity().uuid()
            && self.entity().entity_type() == partner.entity().entity_type()
            && self.is_in_love()
            && partner.is_in_love()
    }

    /// Plays this animal type's eating sound.
    pub fn play_eating_sound(&self) {
        (self.breeding.props.play_eating_sound)(self.entity());
    }

    /// Random breeding experience: 1 to 3 (`Animal.getBaseExperienceReward`).
    #[must_use]
    pub fn base_experience_reward() -> i32 {
        1 + rand::random_range(0..3)
    }

    /// Creates offspring with `partner` and adds it to `world`
    /// (`Animal.spawnChildFromBreeding`).
    pub fn spawn_child_from_breeding(&self, world: &Arc<World>, partner: &Animal<'_>) {
        let Some(offspring) = self.ageable.breed_offspring(world, &partner.ageable) else {
            return;
        };

        let Some(offspring_ageable) = Ageable::of(offspring.as_ref()) else {
            log::error!(
                "breeding entity type {} created non-ageable offspring",
                self.entity().entity_type().key
            );
            return;
        };
        offspring_ageable.set_baby(true);
        if let Err(error) = offspring.try_set_position(self.entity().position()) {
            log::error!(
                "failed to position breeding offspring {} at parent {}: {error}",
                offspring.id(),
                self.entity().id()
            );
            return;
        }
        offspring.set_rotation((0.0, 0.0));
        offspring.set_old_position_to_current();

        self.finalize_spawn_child_from_breeding(world, partner);

        if let Err(error) = world.try_add_entity(offspring) {
            log::error!(
                "failed to add breeding offspring for entity {} to world: {error}",
                self.entity().id()
            );
        }
    }

    /// Breeding side effects on both parents (`Animal.finalizeSpawnChildFromBreeding`).
    fn finalize_spawn_child_from_breeding(&self, world: &Arc<World>, partner: &Animal<'_>) {
        if self.love_cause().or_else(|| partner.love_cause()).is_some() {
            // TODO: Award the animals-bred stat and advancement once those foundations exist.
        }

        self.ageable.set_age(PARENT_AGE_AFTER_BREEDING);
        partner.ageable.set_age(PARENT_AGE_AFTER_BREEDING);
        self.reset_love();
        partner.reset_love();
        self.entity()
            .broadcast_entity_event(EntityStatus::InLoveHearts);

        if world.get_game_rule(&MOB_DROPS) {
            let orb: SharedEntity = Arc::new(ExperienceOrbEntity::with_value(
                &vanilla_entities::EXPERIENCE_ORB,
                next_entity_id(),
                self.entity().position(),
                rand::random_range(BREEDING_XP),
                Arc::downgrade(world),
            ));
            if let Err(error) = world.try_add_entity(orb) {
                log::debug!("failed to add breeding experience orb: {error}");
            }
        }
    }

    /// `Animal.aiStep` after `super`: love mode only lasts for adults and
    /// counts down. Vanilla's periodic heart particles are client-local.
    fn tick_love(&self) {
        if self.ageable.age() != 0 {
            self.reset_love();
            return;
        }
        let mut state = self.breeding.state.lock();
        if state.in_love > 0 {
            state.in_love -= 1;
        }
    }

    /// `Animal.mobInteract`: feeding food starts love mode for adults and
    /// speeds up growth for babies.
    fn feed(&self, mob: &Mob<'_>, ctx: &mut MobInteractCtx<'_>) -> Flow {
        let is_food = {
            let inventory = ctx.player.inventory.lock();
            self.is_food(inventory.get_item_in_hand(ctx.hand))
        };
        if !is_food {
            return Flow::Continue;
        }

        let age = self.ageable.age();
        if age == 0 && self.can_fall_in_love() {
            mob.use_player_item(ctx.player, ctx.hand);
            self.set_in_love(Some(ctx.player));
            self.play_eating_sound();
            ctx.result = InteractionResult::SuccessServer;
            return Flow::Stop;
        }

        if self.ageable.can_age_up() {
            mob.use_player_item(ctx.player, ctx.hand);
            self.ageable
                .age_up(Ageable::speed_up_seconds_when_feeding(-age), true);
            self.play_eating_sound();
            ctx.result = InteractionResult::Success;
            return Flow::Stop;
        }

        Flow::Continue
    }
}

/// Components every animal has; keeps stage bounds short.
pub trait Parts: mob::Parts + Has<Age> + Has<Breeding> {
    /// Returns this entity as an animal.
    fn animal(&self) -> Animal<'_>;
}

impl<E: mob::Parts + Has<Age> + Has<Breeding>> Parts for E {
    fn animal(&self) -> Animal<'_> {
        Animal::new(self, self.get::<Age>(), self.get::<Breeding>())
    }
}

/// Stage running [`Animal::tick_love`].
pub const AI_STEP: StageId = StageId("animal.ai_step");
/// Stage ending love mode for non-adults in `customServerAiStep`.
pub const CUSTOM_SERVER_AI_STEP: StageId = StageId("animal.custom_server_ai_step");
/// Stage feeding the animal in `mobInteract`.
pub const MOB_INTERACT: StageId = StageId("animal.mob_interact");

/// Vanilla's chains for an animal type: `LivingEntity` → `Mob` →
/// `AgeableMob` → `Animal`, base first.
#[must_use]
pub fn definition<E: Parts>() -> MobDefinition<E> {
    MobDefinition {
        ai_step: ai_step(),
        custom_server_ai_step: custom_server_ai_step(),
        mob_interact: mob_interact(),
        finalize_spawn: Pipeline::new([
            ageable::finalize_spawn::<E>(),
            mob::stages::finalize_spawn::<E>(),
        ]),
    }
}

/// Vanilla's animal `aiStep` chain.
#[must_use]
pub fn ai_step<E: Parts>() -> Pipeline<E, AiStep> {
    fn love<E: Parts>(entity: &E) {
        entity.animal().tick_love();
    }
    Pipeline::new([
        mob::stages::ai_step::<E>(),
        ageable::ai_step::<E>(),
        Stage::effect(AI_STEP, love::<E>),
    ])
}

/// Vanilla's animal `customServerAiStep` chain.
#[must_use]
pub fn custom_server_ai_step<E: Parts>() -> Pipeline<E, CustomServerAiStep> {
    fn run<E: Parts>(entity: &E) {
        let animal = entity.animal();
        if animal.ageable.age() != 0 {
            animal.reset_love();
        }
    }
    Pipeline::new([Stage::effect(CUSTOM_SERVER_AI_STEP, run::<E>)])
}

/// Vanilla's animal `mobInteract` chain: feeding, then the age lock.
#[must_use]
pub fn mob_interact<E: Parts>() -> Pipeline<E, MobInteract> {
    fn run<E: Parts>(entity: &E, ctx: &mut MobInteractCtx<'_>) -> Flow {
        entity.animal().feed(&entity.mob(), ctx)
    }
    Pipeline::new([
        Stage::new(MOB_INTERACT, run::<E>),
        ageable::mob_interact::<E>(),
    ])
}

/// How attractive `pos` is as a walk target: grass first, then light
/// (`Animal.getWalkTargetValue`).
fn walk_target_value(entity: &dyn Entity, pos: BlockPos) -> f32 {
    let Some(world) = entity.level() else {
        return 0.0;
    };

    if world.get_block_state(pos.below()).get_block() == &vanilla_blocks::GRASS_BLOCK {
        10.0
    } else {
        world.pathfinding_cost_from_light_levels(pos)
    }
}

/// Returns whether light at `pos` allows animals to spawn naturally.
pub fn is_bright_enough_to_spawn(level: &(impl LevelReader + ?Sized), pos: BlockPos) -> bool {
    level.raw_brightness(pos, 0) > 8
}

/// `Animal.checkAnimalSpawnRules`: the block below must allow animals, and
/// light must suffice unless the spawn reason ignores it.
pub fn check_animal_spawn_rules(
    _entity_type: EntityTypeRef,
    level: &(impl LevelReader + ?Sized),
    spawn_reason: EntitySpawnReason,
    pos: BlockPos,
    _random: &mut dyn Rng,
) -> bool {
    let bright_enough =
        spawn_reason.ignores_light_requirements() || is_bright_enough_to_spawn(level, pos);
    level
        .get_block_state(pos.below())
        .get_block()
        .has_tag(&BlockTag::ANIMALS_SPAWNABLE_ON)
        && bright_enough
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, init_vanilla_registry, vanilla_blocks};
    use steel_utils::BlockStateId;
    use steel_worldgen::density_functions::overworld::OverworldNoiseSettings;

    use super::*;

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
            OverworldNoiseSettings::MIN_Y
        }

        fn height(&self) -> i32 {
            OverworldNoiseSettings::HEIGHT
        }

        fn sea_level(&self) -> i32 {
            OverworldNoiseSettings::SEA_LEVEL
        }
    }

    fn spawn_rule_level(block_below: BlockStateId, raw_brightness: u8) -> SpawnRuleLevel {
        SpawnRuleLevel {
            below_pos: BlockPos::new(0, 63, 0),
            below_state: block_below,
            raw_brightness,
        }
    }

    fn natural_spawn(level: &SpawnRuleLevel, reason: EntitySpawnReason) -> bool {
        check_animal_spawn_rules(
            &vanilla_entities::PIG,
            level,
            reason,
            BlockPos::new(0, 64, 0),
            &mut rand::rng(),
        )
    }

    #[test]
    fn animal_spawn_rules_require_spawnable_block_tag() {
        init_vanilla_registry();
        let level = spawn_rule_level(vanilla_blocks::STONE.default_state(), 15);
        assert!(!natural_spawn(&level, EntitySpawnReason::Natural));
    }

    #[test]
    fn animal_spawn_rules_require_raw_brightness_above_eight() {
        init_vanilla_registry();
        let dark = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 8);
        assert!(!natural_spawn(&dark, EntitySpawnReason::Natural));

        let lit = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 9);
        assert!(natural_spawn(&lit, EntitySpawnReason::Natural));
    }

    #[test]
    fn animal_spawn_rules_trial_spawner_ignores_light() {
        init_vanilla_registry();
        let level = spawn_rule_level(vanilla_blocks::GRASS_BLOCK.default_state(), 0);
        assert!(natural_spawn(&level, EntitySpawnReason::TrialSpawner));
    }
}
