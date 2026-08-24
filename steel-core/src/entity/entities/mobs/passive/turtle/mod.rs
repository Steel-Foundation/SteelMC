//! Vanilla Turtle entity.
//!
//! Turtles are amphibious passive animals that breed with seagrass, return to
//! a home beach to lay eggs, and travel long distances through water. Their AI
//! is the full vanilla goal set, ported in [`goals`].

mod goals;

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::TurtleEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_game_events,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use self::goals::{
    TurtleBreedGoal, TurtleGoHomeGoal, TurtleGoToWaterGoal, TurtleLayEggGoal, TurtlePanicGoal,
    TurtleRandomStrollGoal, TurtleTravelGoal,
};
use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{LookAtPlayerGoal, TemptGoal};
use crate::entity::ai::path::PathType;
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::player::Player;
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

impl Entity for TurtleEntity {
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
        if AgeableMob::is_baby(self) {
            self.entity_type.dimensions.scale(BABY_SCALE * scale)
        } else if self.entity_type.fixed {
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
        SoundSource::Neutral
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        let sound = if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_SHAMBLE_BABY
        } else {
            &sound_events::ENTITY_TURTLE_SHAMBLE
        };
        self.play_sound(sound, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        let home = self.home_pos();
        nbt.insert(
            "home_pos",
            NbtTag::IntArray(vec![home.x(), home.y(), home.z()]),
        );
        nbt.insert("has_egg", self.has_egg());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

        if let Some(home) = nbt.int_array("home_pos")
            && home.len() == 3
        {
            self.set_home_pos(BlockPos::new(home[0], home[1], home[2]));
        } else {
            self.set_home_pos(self.block_position());
        }
        self.set_has_egg(nbt.byte("has_egg").is_some_and(|value| value != 0));
    }
}

impl LivingEntity for TurtleEntity {
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
        Some(if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_HURT_BABY
        } else {
            &sound_events::ENTITY_TURTLE_HURT
        })
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(if AgeableMob::is_baby(self) {
            &sound_events::ENTITY_TURTLE_DEATH_BABY
        } else {
            &sound_events::ENTITY_TURTLE_DEATH
        })
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        self.tick_laying_egg();
        result
    }
}

impl AgeableMob for TurtleEntity {
    fn ageable_base(&self) -> &AgeableMobBase {
        &self.ageable_base
    }

    fn is_age_locked(&self) -> bool {
        *self.entity_data.lock().ageable_mob().age_locked.get()
    }

    fn set_age_locked(&self, age_locked: bool) {
        self.entity_data
            .lock()
            .ageable_mob_mut()
            .age_locked
            .set(age_locked);
    }

    fn set_synced_baby(&self, baby: bool) {
        self.entity_data.lock().ageable_mob_mut().baby.set(baby);
    }

    fn age_boundary_changed(&self, _baby: bool) {
        self.refresh_dimensions();
    }
}

impl Animal for TurtleEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        TurtleEntity::is_food(item_stack)
    }

    fn can_fall_in_love(&self) -> bool {
        self.in_love_time() <= 0 && !self.has_egg()
    }
}

impl Mob for TurtleEntity {
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
        (!self.is_in_water() && self.on_ground() && !AgeableMob::is_baby(self))
            .then_some(&sound_events::ENTITY_TURTLE_AMBIENT_LAND)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.set_home_pos(self.block_position());
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        Animal::mob_interact_animal(self, player, hand)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for TurtleEntity {}

#[cfg(test)]
mod tests;
