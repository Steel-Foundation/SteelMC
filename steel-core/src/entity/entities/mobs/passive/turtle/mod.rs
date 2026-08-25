//! Vanilla Turtle entity.
//!
//! Turtles are amphibious passive animals that breed with seagrass, return to
//! a home beach to lay eggs, and travel long distances through water. Their AI
//! is the full vanilla goal set, ported in [`goals`].

mod goals;
mod traits;

use std::sync::Weak;

use glam::DVec3;
use steel_macros::entity_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::TurtleEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, vanilla_game_events, vanilla_game_rules, vanilla_loot_tables,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use self::goals::{
    TurtleBreedGoal, TurtleGoHomeGoal, TurtleGoToWaterGoal, TurtleLayEggGoal, TurtlePanicGoal,
    TurtleRandomStrollGoal, TurtleTravelGoal,
};
use crate::entity::ai::goal::{LookAtPlayerGoal, TemptGoal};
use crate::entity::ai::path::PathType;
use crate::entity::living_entity::gift_loot_items_with_rng;
use crate::entity::{
    AgeableMobBase, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity,
    LivingEntityBase, MobBase,
};
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Baby turtles render and collide at 0.3 of the adult size.
const BABY_SCALE: f32 = 0.3;
const DEFAULT_STEP_HEIGHT: f32 = 1.0;
/// Vanilla level event 2001: the block-break dust and sound shown as a turtle
/// kicks up sand while laying an egg.
const LAYING_EGG_PARTICLES: i32 = 2001;

#[entity_behavior(class = "Turtle")]
/// Vanilla turtle entity.
pub struct TurtleEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<TurtleEntityData>,
    /// Home beach this turtle returns to in order to lay eggs. Distinct from the
    /// shared mob home restriction, matching vanilla's own `homePos` field.
    home_pos: SyncMutex<BlockPos>,
    /// Whether the go-home goal is currently steering this turtle. Transient,
    /// not persisted, matching vanilla's `goingHome`.
    going_home: SyncMutex<bool>,
    /// The far-water target chosen by the travel goal, if any. Transient,
    /// matching vanilla's nullable `travelPos`.
    travel_pos: SyncMutex<Option<BlockPos>>,
    /// Counts up while an egg is being laid so laying finishes after a delay.
    /// Transient, matching vanilla's `layEggCounter`.
    lay_egg_counter: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `TurtleEntity`.
unsafe impl DowncastType for TurtleEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/turtle");
}

impl TurtleEntity {
    /// Creates a new turtle at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a turtle from persisted base entity state.
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
        Self::initialize_turtle_pathfinding_malus(&mob_base);
        let mut entity_data = TurtleEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            // Goal priorities mirror vanilla `Turtle.registerGoals`.
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, TurtlePanicGoal::new(1.2));
            goal_selector.add_goal(1, TurtleBreedGoal::new(1.0));
            goal_selector.add_goal(1, TurtleLayEggGoal::new(1.0));
            goal_selector.add_goal(
                2,
                TemptGoal::new(
                    1.1,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::TURTLE_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(3, TurtleGoToWaterGoal::new(1.0));
            goal_selector.add_goal(4, TurtleGoHomeGoal::new(1.0));
            goal_selector.add_goal(7, TurtleTravelGoal::new(1.0));
            goal_selector.add_goal(8, LookAtPlayerGoal::new(8.0));
            goal_selector.add_goal(9, TurtleRandomStrollGoal::new(1.0, 100));
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
            home_pos: SyncMutex::new(BlockPos::ZERO),
            going_home: SyncMutex::new(false),
            travel_pos: SyncMutex::new(None),
            lay_egg_counter: SyncMutex::new(0),
        }
    }

    /// Applies the turtle-specific vanilla pathfinding malus overrides: water is
    /// free to path through and doors are impassable.
    // TODO(amphibious-navigation): vanilla turtles swim with a dedicated
    // AmphibiousPathNavigation and a custom TurtleMoveControl (water buoyancy,
    // reduced land speed). Steel has neither yet, so a zero WATER malus on the
    // default navigation is an approximation. Swap to a real amphibious navigator
    // once it lands; frogs, axolotls, and dolphins will want it too.
    fn initialize_turtle_pathfinding_malus(mob_base: &MobBase) {
        let mut malus = mob_base.pathfinding_malus().lock();
        malus.set(PathType::Water, 0.0);
        malus.set(PathType::DoorIronClosed, -1.0);
        malus.set(PathType::DoorWoodClosed, -1.0);
        malus.set(PathType::DoorOpen, -1.0);
    }

    /// Returns whether this turtle is carrying an egg to lay.
    #[must_use]
    pub fn has_egg(&self) -> bool {
        *self.entity_data.lock().has_egg.get()
    }

    pub(crate) fn set_has_egg(&self, has_egg: bool) {
        self.entity_data.lock().has_egg.set(has_egg);
    }

    /// Returns whether this turtle is in the middle of laying its egg.
    #[must_use]
    pub fn is_laying_egg(&self) -> bool {
        *self.entity_data.lock().laying_egg.get()
    }

    /// Starts or stops the egg-laying animation, resetting the lay counter to
    /// match vanilla's `setLayingEgg`.
    pub(crate) fn set_laying_egg(&self, laying: bool) {
        *self.lay_egg_counter.lock() = i32::from(laying);
        self.entity_data.lock().laying_egg.set(laying);
    }

    /// Returns how many ticks the current egg-laying has been running.
    #[must_use]
    pub(crate) fn lay_egg_counter(&self) -> i32 {
        *self.lay_egg_counter.lock()
    }

    /// Advances the egg-laying counter by one tick.
    pub(crate) fn increment_lay_egg_counter(&self) {
        *self.lay_egg_counter.lock() += 1;
    }

    /// Returns whether the go-home goal is currently steering this turtle.
    #[must_use]
    pub(crate) fn going_home(&self) -> bool {
        *self.going_home.lock()
    }

    /// Records whether the go-home goal is currently steering this turtle.
    pub(crate) fn set_going_home(&self, going_home: bool) {
        *self.going_home.lock() = going_home;
    }

    /// Returns the travel goal's current far-water target, if any.
    #[must_use]
    pub(crate) fn travel_pos(&self) -> Option<BlockPos> {
        *self.travel_pos.lock()
    }

    /// Records the travel goal's far-water target.
    pub(crate) fn set_travel_pos(&self, pos: Option<BlockPos>) {
        *self.travel_pos.lock() = pos;
    }

    /// Returns this turtle's home beach position.
    #[must_use]
    pub fn home_pos(&self) -> BlockPos {
        *self.home_pos.lock()
    }

    /// Records the home beach this turtle returns to in order to lay eggs.
    pub fn set_home_pos(&self, pos: BlockPos) {
        *self.home_pos.lock() = pos;
    }

    /// Returns whether an item stack matches the vanilla turtle food tag (seagrass).
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::TURTLE_FOOD)
    }

    /// Vanilla `Turtle.ageBoundaryReached`: a turtle that grows into an adult
    /// sheds a scute, rolled from the turtle grow gift loot table, when the
    /// `mobDrops` game rule is enabled.
    fn drop_turtle_scute(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.get_game_rule(&vanilla_game_rules::MOB_DROPS) {
            return;
        }

        let drops = {
            let mut rng = rand::rng();
            gift_loot_items_with_rng(self, &vanilla_loot_tables::GAMEPLAY_TURTLE_GROW, &mut rng)
        };
        for item_stack in drops {
            self.spawn_at_location(item_stack, 0.0);
        }
    }

    /// Emits the vanilla sand-kicking particles and game event every five ticks
    /// while an egg is being laid, matching `Turtle.aiStep`.
    fn tick_laying_egg(&self) {
        if !LivingEntity::is_alive(self)
            || !self.is_laying_egg()
            || self.lay_egg_counter() < 1
            || self.lay_egg_counter() % 5 != 0
        {
            return;
        }

        let pos = self.block_position();
        let Some(world) = self.level() else {
            return;
        };
        let below = world.get_block_state(pos.below());
        if !below.get_block().has_tag(&BlockTag::SAND) {
            return;
        }

        world.level_event(LAYING_EGG_PARTICLES, pos, i32::from(below.0), None);
        world.game_event(
            &vanilla_game_events::ENTITY_ACTION,
            pos,
            &GameEventContext::new(Some(self), None),
        );
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
}

#[cfg(test)]
mod tests;
