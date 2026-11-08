//! Vanilla Chicken entity with variant + sound-variant parity, wing-flap
//! slow-fall, and periodic egg-laying behavior.

use crate::entity::component::definition::BaseExperienceRewardCtx;
use crate::entity::component::definition::PositionRiderCtx;
use crate::entity::component::pipeline::Flow;
use crate::entity::prelude::*;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::chicken_sound_variant::{
    ChickenAge, ChickenSoundVariantRef, ChickenSoundVariantRegistry,
};
use steel_registry::chicken_variant::{ChickenVariantRef, ChickenVariantRegistry};
use steel_registry::data_components::vanilla_components::{CHICKEN_SOUND_VARIANT, CHICKEN_VARIANT};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_entity_data::ChickenEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_loot_tables;
use steel_registry::{
    REGISTRY, RegistryReference, TaggedRegistryExt, sound_events, vanilla_game_events,
};
use steel_utils::locks::CheckedMutex;
use steel_utils::{BlockPos, BlockStateId};

use crate::entity::ageable::{self, Age, AgeableProps};
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::path::PathType;
use crate::entity::animal::{self, Breeding, BreedingProps};
use crate::entity::component::definition::Definition;
use crate::entity::component::pipeline::{Stage, StageId};
use crate::entity::variant::{self, Variants};
use crate::entity::{
    DefaultEntityPush, Entity, EntityBase, LivingEntityBase, LivingProps, MobBase, MobProps,
    SpawnRule,
};
use crate::entity::{EntityProps, mob};

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
#[entity(key = "steel:entity/chicken", constructors, definition = Self::define)]
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

    const RIDER_FACING: StageId = StageId("chicken.rider_facing");
    const JOCKEY_XP: StageId = StageId("chicken.jockey_xp");

    /// Vanilla `Chicken`: the animal chains plus flapping and egg laying
    /// after `Animal.aiStep`, and variant selection before
    /// `AgeableMob.finalizeSpawn`.
    fn define() -> Definition<Self> {
        let mut definition = animal::definition::<Self>();
        // `Chicken.getBaseExperienceReward`: jockeys give 10 instead of `super`.
        definition.base_experience_reward = definition.base_experience_reward.before(
            animal::BASE_EXPERIENCE_REWARD,
            [Stage::new(
                Self::JOCKEY_XP,
                |chicken: &Self, ctx: &mut BaseExperienceRewardCtx<'_>| {
                    if chicken.is_chicken_jockey() {
                        ctx.result = 10;
                        return Flow::Stop;
                    }
                    Flow::Continue
                },
            )],
        );
        definition.position_rider = definition.position_rider.then([Stage::new(
            Self::RIDER_FACING,
            Self::position_rider_after_super,
        )]);
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
        6000 + rand::random_range(0..6000)
    }

    /// Runs vanilla `Chicken.aiStep` wing-flap and slow-fall side effects.
    fn tick_flapping(&self) {
        let on_ground = self.on_ground();
        let velocity = self.velocity();

        {
            let mut state = self.state.lock();
            state.o_flap = state.flap;
            state.o_flap_speed = state.flap_speed;
            let adjustment = if on_ground { -1.0 } else { 4.0 };
            state.flap_speed = (state.flap_speed + adjustment * 0.3).clamp(0.0, 1.0);
            if !on_ground && state.flapping < 1.0 {
                state.flapping = 1.0;
            }
            state.flapping *= 0.9;
            state.flap += state.flapping * 2.0;
        }

        if !on_ground && velocity.y < 0.0 {
            self.set_velocity(DVec3::new(velocity.x, velocity.y * 0.6, velocity.z));
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
                let pitch = 1.0 + (rand::random::<f32>() - rand::random::<f32>()) * 0.2;
                self.play_sound(&sound_events::ENTITY_CHICKEN_EGG, 1.0, pitch);
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
        apply_implicit_item_components: Self::entity_apply_implicit_item_components,
        play_step_sound: Self::entity_play_step_sound,
        is_flapping: Self::entity_is_flapping,
        on_flap: Self::entity_on_flap,
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
        state.next_flap = fly_dist + state.flap_speed / 2.0;
    }

    /// `Chicken.positionRider` after `super`: the rider faces the chicken's way.
    fn position_rider_after_super(&self, ctx: &mut PositionRiderCtx<'_>) -> Flow {
        if let Some(living) = ctx.passenger.as_living_entity() {
            living.set_y_body_rot(self.living().y_body_rot());
        }
        Flow::Continue
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
