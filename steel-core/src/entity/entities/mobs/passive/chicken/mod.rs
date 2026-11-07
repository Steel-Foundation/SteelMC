//! Vanilla Chicken entity with variant + sound-variant parity, wing-flap
//! slow-fall, and periodic egg-laying behavior.

use crate::entity::LivingParts as _;
use crate::entity::prelude::*;
use std::sync::{LazyLock, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::chicken_sound_variant::{
    ChickenAge, ChickenSoundVariantRef, ChickenSoundVariantRegistry,
};
use steel_registry::chicken_variant::{ChickenVariantRef, ChickenVariantRegistry};
use steel_registry::data_components::vanilla_components::{CHICKEN_SOUND_VARIANT, CHICKEN_VARIANT};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ChickenEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_loot_tables;
use steel_registry::{
    REGISTRY, RegistryReference, TaggedRegistryExt, sound_events, vanilla_game_events,
};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

use crate::entity::ageable::{self, Age, AgeableProps};
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::path::PathType;
use crate::entity::animal::{self, Animal, Breeding, BreedingProps, Parts as _};
use crate::entity::component::definition::MobDefinition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, Entity, EntityBase, EntityBaseLoad, EntityPose, LivingEntityBase,
    LivingProps, MobBase, MobProps, SpawnRule, position_rider_default,
};
use crate::entity::{EntityProps, mob};
use crate::world::World;

const CHICKEN_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.375, 0.0)];
const CHICKEN_BABY_WIDTH: f32 = 0.3;
const CHICKEN_BABY_HEIGHT: f32 = 0.4;
const CHICKEN_BABY_EYE_HEIGHT: f32 = 0.28125;

const CHICKEN_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    CHICKEN_BABY_WIDTH,
    CHICKEN_BABY_HEIGHT,
    CHICKEN_BABY_EYE_HEIGHT,
    EntityAttachments::new(&CHICKEN_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);

/// Flap speed gained per tick while airborne (vanilla `Chicken.aiStep`).
const FLAP_SPEED_AIR_GAIN: f32 = 4.0;
/// Flap speed lost per tick while grounded (vanilla `Chicken.aiStep`).
const FLAP_SPEED_GROUND_LOSS: f32 = 1.0;
/// Scales the per-tick flap-speed adjustment (vanilla `Chicken.aiStep`).
const FLAP_SPEED_ADJUST_SCALE: f32 = 0.3;
const FLAP_SPEED_MIN: f32 = 0.0;
const FLAP_SPEED_MAX: f32 = 1.0;
/// Flapping strength restored whenever the chicken is airborne.
const MIN_FLAPPING_STRENGTH: f32 = 1.0;
/// Multiplicative flapping-strength decay per tick.
const FLAPPING_STRENGTH_DECAY: f32 = 0.9;
/// Scales accumulated flap rotation (vanilla `Chicken.aiStep`).
const FLAP_ROTATION_SCALE: f32 = 2.0;
/// Downward velocity multiplier while airborne, producing the chicken slow-fall.
const FALL_DRAG_Y: f32 = 0.6;
/// Divisor converting flap speed into the next flap distance threshold.
const NEXT_FLAP_SPEED_DIVISOR: f32 = 2.0;

/// Minimum ticks between egg lays (vanilla `nextInt(6000) + 6000`).
const EGG_LAY_MIN_DELAY_TICKS: i32 = 6000;
/// Range of the randomized egg-lay delay in ticks.
const EGG_LAY_RANDOM_RANGE_TICKS: i32 = 6000;
const EGG_LAY_SOUND_VOLUME: f32 = 1.0;
const EGG_LAY_SOUND_BASE_PITCH: f32 = 1.0;
/// Pitch jitter around the base egg-lay sound pitch.
const EGG_LAY_SOUND_PITCH_JITTER: f32 = 0.2;
/// Offset at which laid eggs spawn above the chicken's feet.
/// Experience rewarded for a chicken jockey (vanilla `Chicken.getBaseExperienceReward`).
const CHICKEN_JOCKEY_EXPERIENCE_REWARD: i32 = 10;

/// Runtime state unique to chickens: wing-flap animation, egg timer, and jockey flag.
#[derive(Debug, Clone, Copy)]
struct ChickenState {
    flap: f32,
    flap_speed: f32,
    o_flap: f32,
    o_flap_speed: f32,
    flapping: f32,
    next_flap: f32,
    egg_time: i32,
    is_chicken_jockey: bool,
}

impl ChickenState {
    const fn new() -> Self {
        Self {
            flap: 0.0,
            flap_speed: 0.0,
            o_flap: 0.0,
            o_flap_speed: 0.0,
            flapping: 1.0,
            next_flap: 1.0,
            egg_time: 0,
            is_chicken_jockey: false,
        }
    }
}

/// Vanilla chicken entity.
#[entity_behavior(class = "Chicken", spawn_rule)]
#[derive(Entity)]
#[entity(hooks = Self::ENTITY, save = Self::save_chicken, load = Self::load_chicken)]
pub struct ChickenEntity {
    #[entity(base)]
    base: EntityBase,
    #[entity(entity_type)]
    entity_type: EntityTypeRef,
    #[entity(component)]
    living_base: LivingEntityBase,
    #[entity(component)]
    mob_base: MobBase,
    #[entity(component)]
    age: Age,
    #[entity(component)]
    breeding: Breeding,
    state: CheckedMutex<ChickenState>,
    #[entity(synced)]
    entity_data: CheckedMutex<ChickenEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChickenEntity`.
unsafe impl DowncastType for ChickenEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/chicken");
}

impl ChickenEntity {
    /// Vanilla `SpawnPlacements` predicate: `Animal.checkAnimalSpawnRules`.
    pub(crate) const SPAWN_RULE: SpawnRule = |entity_type, level, spawn_reason, pos, random| {
        animal::check_animal_spawn_rules(entity_type, level, spawn_reason, pos, random)
    };

    const AGEABLE: AgeableProps = AgeableProps {
        breed_offspring: variant::breed_offspring::<Self>,
    };
    const BREEDING: BreedingProps = BreedingProps::new(Self::is_food);
    const LIVING: LivingProps<Self> = LivingProps {
        hurt_sound: |chicken, _| Some(chicken.current_sound_set().hurt_sound),
        death_sound: |chicken| Some(chicken.current_sound_set().death_sound),
        chicken_loot_variant: |chicken| Some(&chicken.variant().key),
        base_experience_reward: |chicken| {
            if chicken.is_chicken_jockey() {
                CHICKEN_JOCKEY_EXPERIENCE_REWARD
            } else {
                Animal::base_experience_reward()
            }
        },
        ..animal::living_props()
    };
    const MOB: MobProps<Self> = MobProps {
        ambient_sound: |chicken| Some(chicken.current_sound_set().ambient_sound),
        // Chicken jockeys persist like their rider.
        remove_when_far_away: |chicken, _| chicken.is_chicken_jockey(),
        ..animal::mob_props()
    };
    const GOALS: StageId = StageId("chicken.goals");
    const WATER_MALUS: StageId = StageId("chicken.water_malus");
    const FLAP: StageId = StageId("chicken.flap");
    const LAY_EGG: StageId = StageId("chicken.lay_egg");

    /// Vanilla `Chicken`: the animal chains plus flapping and egg laying
    /// after `Animal.aiStep`, and variant selection before
    /// `AgeableMob.finalizeSpawn`.
    fn define() -> MobDefinition<Self> {
        let mut definition = animal::definition::<Self>();
        // `Mob`'s constructor registers goals before `Animal`'s sets its malus.
        definition.construct = definition
            .construct
            .before(
                animal::PATHFINDING_MALUS,
                [Stage::effect(Self::GOALS, Self::register_goals)],
            )
            .then([Stage::effect(Self::WATER_MALUS, |mob_base: &MobBase| {
                // Chickens do not avoid water.
                mob_base
                    .pathfinding_malus()
                    .lock()
                    .set(PathType::Water, 0.0);
            })]);
        definition.ai_step = definition.ai_step.then([
            Stage::effect(Self::FLAP, Self::tick_flapping),
            Stage::effect(Self::LAY_EGG, Self::tick_egg_laying),
        ]);
        definition.finalize_spawn = definition.finalize_spawn.before(
            ageable::FINALIZE_SPAWN,
            [variant::pick_spawn_variants::<Self>()],
        );
        definition
    }

    fn definition() -> &'static MobDefinition<Self> {
        static DEFINITION: LazyLock<MobDefinition<ChickenEntity>> =
            LazyLock::new(ChickenEntity::define);
        &DEFINITION
    }

    /// Creates a new chicken at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::from_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a chicken from persisted base entity state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::from_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn from_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let mut entity_data = ChickenEntityData::new();
        let living_base = LivingEntityBase::new(entity_type, &Self::LIVING, &mut entity_data);
        let mob_base = MobBase::new(&Self::MOB, Self::definition());
        let mut state = ChickenState::new();
        state.egg_time = Self::roll_egg_lay_time();

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            age: Age::new(&Self::AGEABLE),
            breeding: Breeding::new(&Self::BREEDING),
            state: CheckedMutex::new(state),
            entity_data: CheckedMutex::new(entity_data),
        }
    }

    /// `Chicken.registerGoals`.
    fn register_goals(mob_base: &MobBase) {
        let mut goal_selector = mob_base.goal_selector().lock();
        goal_selector.add_goal(0, FloatGoal::new(mob_base));
        goal_selector.add_goal(1, PanicGoal::new(1.4));
        goal_selector.add_goal(2, BreedGoal::new(1.0));
        goal_selector.add_goal(3, TemptGoal::new(1.0, Self::is_food, false));
        goal_selector.add_goal(4, FollowParentGoal::new(1.1));
        goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(1.0));
        goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
        goal_selector.add_goal(7, RandomLookAroundGoal::new());
    }

    /// Returns the sound set for this chicken's current age (vanilla `Chicken.getSoundSet`).
    fn current_sound_set(&self) -> &'static ChickenAge {
        let sound_variant = self.sound_variant();
        if self.animal().ageable().is_baby() {
            &sound_variant.baby_sounds
        } else {
            &sound_variant.adult_sounds
        }
    }

    /// Returns whether an item stack matches the vanilla chicken food tag.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::CHICKEN_FOOD)
    }

    /// Returns whether this chicken is currently ridden by a chicken jockey mob.
    #[must_use]
    pub fn is_chicken_jockey(&self) -> bool {
        self.state.lock().is_chicken_jockey
    }

    /// Sets whether this chicken is a chicken jockey mount.
    pub fn set_chicken_jockey(&self, is_chicken_jockey: bool) {
        self.state.lock().is_chicken_jockey = is_chicken_jockey;
    }

    /// Returns the remaining ticks before the next egg is laid.
    fn egg_time(&self) -> i32 {
        self.state.lock().egg_time
    }

    /// Sets the remaining ticks before the next egg is laid.
    fn set_egg_time(&self, egg_time: i32) {
        self.state.lock().egg_time = egg_time;
    }

    /// Rolls the vanilla randomized delay before the next egg lay.
    fn roll_egg_lay_time() -> i32 {
        EGG_LAY_MIN_DELAY_TICKS + rand::random_range(0..EGG_LAY_RANDOM_RANGE_TICKS)
    }

    /// Runs vanilla `Chicken.aiStep` wing-flap and slow-fall side effects.
    fn tick_flapping(&self) {
        let on_ground = self.on_ground();
        let velocity = self.velocity();

        {
            let mut state = self.state.lock();
            state.o_flap = state.flap;
            state.o_flap_speed = state.flap_speed;
            let adjustment = if on_ground {
                -FLAP_SPEED_GROUND_LOSS
            } else {
                FLAP_SPEED_AIR_GAIN
            };
            state.flap_speed = (state.flap_speed + adjustment * FLAP_SPEED_ADJUST_SCALE)
                .clamp(FLAP_SPEED_MIN, FLAP_SPEED_MAX);
            if !on_ground && state.flapping < MIN_FLAPPING_STRENGTH {
                state.flapping = MIN_FLAPPING_STRENGTH;
            }
            state.flapping *= FLAPPING_STRENGTH_DECAY;
            state.flap += state.flapping * FLAP_ROTATION_SCALE;
        }

        if !on_ground && velocity.y < 0.0 {
            self.set_velocity(DVec3::new(
                velocity.x,
                velocity.y * f64::from(FALL_DRAG_Y),
                velocity.z,
            ));
        }
    }

    /// Runs vanilla `Chicken.aiStep` egg-laying side effects.
    fn tick_egg_laying(&self) {
        if self.level().is_none() || !Entity::is_alive(self) || self.animal().ageable().is_baby() {
            return;
        }

        let should_lay_egg = {
            let mut state = self.state.lock();
            // Vanilla gates egg laying on `!isChickenJockey()`.
            if state.is_chicken_jockey {
                return;
            }
            state.egg_time -= 1;
            state.egg_time <= 0
        };

        if should_lay_egg {
            if self
                .living()
                .drop_from_gift_loot_table(&vanilla_loot_tables::GAMEPLAY_CHICKEN_LAY)
            {
                let pitch = EGG_LAY_SOUND_BASE_PITCH
                    + (rand::random::<f32>() - rand::random::<f32>()) * EGG_LAY_SOUND_PITCH_JITTER;
                self.play_sound(
                    &sound_events::ENTITY_CHICKEN_EGG,
                    EGG_LAY_SOUND_VOLUME,
                    pitch,
                );
                self.game_event(&vanilla_game_events::ENTITY_PLACE);
            }
            self.set_egg_time(Self::roll_egg_lay_time());
        }
    }
}

impl Variants for ChickenEntity {
    type VariantRegistry = ChickenVariantRegistry;
    type SoundVariantRegistry = ChickenSoundVariantRegistry;

    fn variant_registry() -> &'static ChickenVariantRegistry {
        &REGISTRY.chicken_variants
    }

    fn sound_variant_registry() -> &'static ChickenSoundVariantRegistry {
        &REGISTRY.chicken_sound_variants
    }

    fn variant(&self) -> ChickenVariantRef {
        self.entity_data.lock().variant.get().value()
    }

    fn set_variant(&self, variant: ChickenVariantRef) {
        self.entity_data
            .lock()
            .variant
            .set(RegistryReference::new(variant));
    }

    fn sound_variant(&self) -> ChickenSoundVariantRef {
        self.entity_data.lock().sound_variant.get().value()
    }

    fn set_sound_variant(&self, sound_variant: ChickenSoundVariantRef) {
        self.entity_data
            .lock()
            .sound_variant
            .set(RegistryReference::new(sound_variant));
    }
}

impl DefaultEntityPush for ChickenEntity {}

impl ChickenEntity {
    const ENTITY: EntityProps<Self> = EntityProps {
        is_hard_collision_relevant: |_| false,
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        dimensions_for_pose: Self::entity_dimensions_for_pose,
        play_step_sound: Self::entity_play_step_sound,
        is_flapping: Self::entity_is_flapping,
        on_flap: Self::entity_on_flap,
        position_rider: Self::entity_position_rider,
        ..mob::entity_props()
    };

    fn entity_apply_implicit_item_components(&self, item_stack: &ItemStack) {
        if let Some(variant) = item_stack.get(CHICKEN_VARIANT) {
            self.set_variant(variant.value());
        }
        if let Some(sound_variant) = item_stack.get(CHICKEN_SOUND_VARIANT) {
            self.set_sound_variant(sound_variant.value());
        }
    }

    fn entity_dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = self.living().scale();
        if self.animal().ageable().is_baby() {
            CHICKEN_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn entity_play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.current_sound_set().step_sound, 0.15, 1.0);
    }

    fn entity_is_flapping(&self) -> bool {
        let fly_dist = self.base().movement_progress().fly_dist();
        fly_dist > self.state.lock().next_flap
    }

    fn entity_on_flap(&self) {
        let fly_dist = self.base().movement_progress().fly_dist();
        let mut state = self.state.lock();
        state.next_flap = fly_dist + state.flap_speed / NEXT_FLAP_SPEED_DIVISOR;
    }

    fn entity_position_rider(&self, passenger: &dyn Entity) {
        position_rider_default(self, passenger);
        if let Some(living) = passenger.as_living_entity() {
            living.set_y_body_rot(self.living().y_body_rot());
        }
    }

    fn save_chicken(&self, nbt: &mut NbtCompound) {
        nbt.insert("IsChickenJockey", i8::from(self.is_chicken_jockey()));
        nbt.insert("EggLayTime", self.egg_time());
        variant::save(self, nbt);
    }

    fn load_chicken(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_chicken_jockey(nbt.byte("IsChickenJockey").is_some_and(|value| value != 0));
        if let Some(egg_time) = nbt.int("EggLayTime") {
            self.set_egg_time(egg_time);
        }
        variant::load(self, nbt);
    }
}

#[cfg(test)]
mod tests;
