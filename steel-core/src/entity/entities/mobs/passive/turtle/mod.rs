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
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::TurtleEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, level_events, vanilla_game_events, vanilla_game_rules,
    vanilla_loot_tables,
};
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, DowncastType, DowncastTypeKey};

use self::goals::closer_to_center_than;
use self::goals::{
    TurtleBreedGoal, TurtleGoHomeGoal, TurtleGoToWaterGoal, TurtleLayEggGoal, TurtlePanicGoal,
    TurtleRandomStrollGoal, TurtleTravelGoal,
};
use crate::entity::ai::goal::{LookAtPlayerGoal, TemptGoal};
use crate::entity::ai::path::PathType;
use crate::entity::living_entity::gift_loot_items_with_rng;
use crate::entity::{
    AgeableMob, AgeableMobBase, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntitySyncedData,
    LivingEntity, LivingEntityBase, Mob, MobBase,
};
use crate::world::World;
use crate::world::game_event::GameEventContext;

const BABY_SCALE: f32 = 0.3;
const ADULT_SCALE: f32 = 1.0;

/// Vanilla `Turtle.BABY_DIMENSIONS` passenger point: the adult's full height,
/// back a quarter block, before baby scaling.
const TURTLE_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.4, -0.25)];
/// Vanilla `Turtle.BABY_DIMENSIONS`, scaled by [`BABY_SCALE`]. The adult figures
/// are inlined because a `const` cannot read the generated entity type.
const TURTLE_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    1.2,
    0.4,
    0.34,
    EntityAttachments::new(&TURTLE_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);
const DEFAULT_STEP_HEIGHT: f32 = 1.0;
/// Vanilla `Turtle.aiStep`: sand particles every 5th tick while laying.
const LAYING_EGG_EMIT_INTERVAL: i32 = 5;

// Vanilla `Turtle.travelInWater` swim physics.
const SWIM_PUSH: f32 = 0.1;
const SWIM_DRAG: f64 = 0.9;
const SWIM_SINK_SPEED: f64 = 0.005;
const SWIM_SINK_HOME_DISTANCE: f64 = 20.0;
const SWIM_LIFT: f64 = 0.005;
const FAR_FROM_HOME_DISTANCE: f64 = 16.0;
const FAR_FROM_HOME_SPEED_DIVISOR: f32 = 2.0;
const FAR_FROM_HOME_MIN_SPEED: f32 = 0.08;
const BABY_SWIM_SPEED_DIVISOR: f32 = 3.0;
const BABY_MIN_SWIM_SPEED: f32 = 0.06;

// Vanilla `TurtleMoveControl` land and steering speed.
const LAND_SPEED_DIVISOR: f32 = 2.0;
const LAND_MIN_SPEED: f32 = 0.06;
const SPEED_LERP: f32 = 0.125;
const CLIMB_SPEED_SHARE: f64 = 0.1;
const ARRIVED_DISTANCE: f64 = 1.0e-5;

/// Vanilla `Turtle.nextStep`: shorter than the usual one-block stride.
const NEXT_STEP_DISTANCE: f32 = 0.15;
/// Vanilla `Turtle.playSwimSound` volume multiplier.
const SWIM_SOUND_VOLUME_SCALE: f32 = 1.5;
/// Vanilla `Turtle.getAmbientSoundInterval`: 10 seconds.
const AMBIENT_SOUND_INTERVAL: i32 = 200;
/// Vanilla `Turtle.checkTurtleSpawnRules`: max height above sea level.
const SPAWN_HEIGHT_ABOVE_SEA_LEVEL: i32 = 4;
/// Vanilla `Turtle.getWalkTargetValue`: score for preferred ground.
const PREFERRED_WALK_TARGET_VALUE: f32 = 10.0;

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
    /// Home beach, vanilla's `homePos` (separate from the shared mob home).
    home_pos: SyncMutex<BlockPos>,
    /// Vanilla `goingHome`, transient.
    going_home: SyncMutex<bool>,
    /// Vanilla `travelPos`, transient.
    travel_pos: SyncMutex<Option<BlockPos>>,
    /// Vanilla `layEggCounter`, transient.
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

    /// Water is free to path through, doors are impassable.
    // TODO(amphibious-navigation): vanilla turtles path with a dedicated
    // AmphibiousPathNavigation. Steel has none yet, so a zero WATER malus on the
    // default navigation approximates it; frogs, axolotls and dolphins will want
    // the real thing. The move control and water travel are ported (see
    // `trim_turtle_speed` and `tick_move_control` / `travel_in_water`); still
    // missing is the turtle's `isStableDestination` check.
    fn initialize_turtle_pathfinding_malus(mob_base: &MobBase) {
        let mut malus = mob_base.pathfinding_malus().lock();
        malus.set(PathType::Water, 0.0);
        malus.set(PathType::DoorIronClosed, -1.0);
        malus.set(PathType::DoorWoodClosed, -1.0);
        malus.set(PathType::DoorOpen, -1.0);
    }

    /// Whether this turtle is carrying an egg to lay.
    #[must_use]
    pub fn has_egg(&self) -> bool {
        *self.entity_data.lock().has_egg.get()
    }

    pub(crate) fn set_has_egg(&self, has_egg: bool) {
        self.entity_data.lock().has_egg.set(has_egg);
    }

    /// Whether this turtle is in the middle of laying its egg.
    #[must_use]
    pub fn is_laying_egg(&self) -> bool {
        *self.entity_data.lock().laying_egg.get()
    }

    /// Vanilla `setLayingEgg`, which also resets the lay counter.
    pub(crate) fn set_laying_egg(&self, laying: bool) {
        *self.lay_egg_counter.lock() = i32::from(laying);
        self.entity_data.lock().laying_egg.set(laying);
    }

    #[must_use]
    pub(crate) fn lay_egg_counter(&self) -> i32 {
        *self.lay_egg_counter.lock()
    }

    pub(crate) fn increment_lay_egg_counter(&self) {
        *self.lay_egg_counter.lock() += 1;
    }

    #[must_use]
    pub(crate) fn going_home(&self) -> bool {
        *self.going_home.lock()
    }

    pub(crate) fn set_going_home(&self, going_home: bool) {
        *self.going_home.lock() = going_home;
    }

    #[must_use]
    pub(crate) fn travel_pos(&self) -> Option<BlockPos> {
        *self.travel_pos.lock()
    }

    pub(crate) fn set_travel_pos(&self, pos: Option<BlockPos>) {
        *self.travel_pos.lock() = pos;
    }

    /// This turtle's home beach.
    #[must_use]
    pub fn home_pos(&self) -> BlockPos {
        *self.home_pos.lock()
    }

    /// Records this turtle's home beach.
    pub fn set_home_pos(&self, pos: BlockPos) {
        *self.home_pos.lock() = pos;
    }

    /// Whether an item stack is turtle food (`#turtle_food`, seagrass).
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::TURTLE_FOOD)
    }

    /// Vanilla `Turtle.ageBoundaryReached`: drop a scute on growing up, gated on
    /// `mobDrops`.
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

    /// Vanilla `TurtleMoveControl.updateSpeed`: trim carried speed, float while
    /// swimming. Runs before the steering, which then eases back up from it.
    fn trim_turtle_speed(&self) {
        if self.is_in_water() {
            let mut velocity = self.velocity();
            velocity.y += SWIM_LIFT;
            self.set_velocity(velocity);

            if !closer_to_center_than(self.home_pos(), self.position(), FAR_FROM_HOME_DISTANCE) {
                self.set_mob_speed(
                    (self.get_speed() / FAR_FROM_HOME_SPEED_DIVISOR).max(FAR_FROM_HOME_MIN_SPEED),
                );
            }
            if AgeableMob::is_baby(self) {
                self.set_mob_speed(
                    (self.get_speed() / BABY_SWIM_SPEED_DIVISOR).max(BABY_MIN_SWIM_SPEED),
                );
            }
        } else if self.on_ground() {
            self.set_mob_speed((self.get_speed() / LAND_SPEED_DIVISOR).max(LAND_MIN_SPEED));
        }
    }

    /// Vanilla `Turtle.aiStep`: sand particles and a game event while laying.
    fn tick_laying_egg(&self) {
        if !LivingEntity::is_alive(self)
            || !self.is_laying_egg()
            || self.lay_egg_counter() < 1
            || self.lay_egg_counter() % LAYING_EGG_EMIT_INTERVAL != 0
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

        world.level_event(
            level_events::PARTICLES_DESTROY_BLOCK,
            pos,
            level_events::encode_block_state_data(u32::from(below.0)),
            None,
        );
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
