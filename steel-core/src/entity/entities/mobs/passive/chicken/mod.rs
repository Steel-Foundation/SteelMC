//! Vanilla Chicken entity with variant + sound-variant parity, wing-flap
//! slow-fall, and periodic egg-laying behavior.

use std::str::FromStr;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::chicken_sound_variant::{ChickenAge, ChickenSoundVariantRef};
use steel_registry::chicken_variant::ChickenVariantRef;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::loot_table::{LootContext, LootTableRef};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::ChickenEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::vanilla_loot_tables;
use steel_registry::{
    REGISTRY, RegistryExt, RegistryReference, TaggedRegistryExt, sound_events, vanilla_game_events,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::path::PathType;
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData, entity_loot_ref, position_rider_default,
};
use crate::physics::MoveResult;
use crate::player::Player;
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
const EGG_LAY_SPAWN_Y_OFFSET: f64 = 0.0;

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
#[entity_behavior(class = "Chicken")]
pub struct ChickenEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    chicken_state: SyncMutex<ChickenState>,
    entity_data: SyncMutex<ChickenEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ChickenEntity`.
unsafe impl DowncastType for ChickenEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/chicken");
}

impl ChickenEntity {
    /// Creates a new chicken at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a chicken from persisted base entity state.
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
        // Chickens do not avoid water (`setPathfindingMalus(PathType.WATER, 0.0F)`).
        mob_base
            .pathfinding_malus()
            .lock()
            .set(PathType::Water, 0.0);
        let mut chicken_state = ChickenState::new();
        chicken_state.egg_time = Self::roll_egg_lay_time();
        let mut entity_data = ChickenEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            // Keep vanilla Chicken goal priorities and speeds in the same order.
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.4));
            goal_selector.add_goal(2, BreedGoal::new(1.0));
            goal_selector.add_goal(
                3,
                TemptGoal::new(
                    1.0,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::CHICKEN_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(4, FollowParentGoal::new(1.1));
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(6, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(7, RandomLookAroundGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            chicken_state: SyncMutex::new(chicken_state),
            entity_data: SyncMutex::new(entity_data),
        }
    }

    /// Sets the active chicken variant by registry entry.
    pub fn set_variant(&self, variant: ChickenVariantRef) {
        self.entity_data
            .lock()
            .variant
            .set(RegistryReference::new(variant));
    }

    /// Returns the active chicken variant, falling back to temperate when invalid.
    #[must_use]
    pub fn variant(&self) -> ChickenVariantRef {
        self.entity_data.lock().variant.get().value()
    }

    /// Sets the active chicken sound variant by registry entry.
    pub fn set_sound_variant(&self, sound_variant: ChickenSoundVariantRef) {
        self.entity_data
            .lock()
            .sound_variant
            .set(RegistryReference::new(sound_variant));
    }

    /// Returns the active chicken sound variant, falling back to classic when invalid.
    #[must_use]
    pub fn sound_variant(&self) -> ChickenSoundVariantRef {
        self.entity_data.lock().sound_variant.get().value()
    }

    fn set_variant_by_key(&self, key: &Identifier) -> bool {
        let Some(variant) = REGISTRY.chicken_variants.by_key(key) else {
            return false;
        };
        self.set_variant(variant);
        true
    }

    fn set_sound_variant_by_key(&self, key: &Identifier) {
        if let Some(sound_variant) = REGISTRY.chicken_sound_variants.by_key(key) {
            self.set_sound_variant(sound_variant);
        }
    }

    /// Returns the sound set for this chicken's current age (vanilla `Chicken.getSoundSet`).
    fn current_sound_set(&self) -> &'static ChickenAge {
        let sound_variant = self.sound_variant();
        if AgeableMob::is_baby(self) {
            &sound_variant.baby_sounds
        } else {
            &sound_variant.adult_sounds
        }
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

    /// Returns whether an item stack matches the vanilla chicken food tag.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::CHICKEN_FOOD)
    }

    /// Returns vanilla `Chicken.isChickenJockey`.
    #[must_use]
    pub fn is_chicken_jockey(&self) -> bool {
        self.chicken_state.lock().is_chicken_jockey
    }

    /// Sets vanilla `Chicken.isChickenJockey`.
    pub fn set_chicken_jockey(&self, is_chicken_jockey: bool) {
        self.chicken_state.lock().is_chicken_jockey = is_chicken_jockey;
    }

    /// Returns the remaining ticks before the next egg is laid.
    fn egg_time(&self) -> i32 {
        self.chicken_state.lock().egg_time
    }

    /// Sets the remaining ticks before the next egg is laid.
    fn set_egg_time(&self, egg_time: i32) {
        self.chicken_state.lock().egg_time = egg_time;
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
            let mut state = self.chicken_state.lock();
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
        if self.level().is_none() || !Entity::is_alive(self) || AgeableMob::is_baby(self) {
            return;
        }

        let should_lay_egg = {
            let mut state = self.chicken_state.lock();
            // Vanilla gates egg laying on `!isChickenJockey()`.
            if state.is_chicken_jockey {
                return;
            }
            state.egg_time -= 1;
            state.egg_time <= 0
        };

        if should_lay_egg {
            if self.drop_gift_loot_table(&vanilla_loot_tables::GAMEPLAY_CHICKEN_LAY) {
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

    /// Rolls a gift loot table at this entity's position and drops each result.
    ///
    /// Mirrors vanilla `LivingEntity.dropFromGiftLootTable` for the
    /// `gameplay/chicken_lay` table, returning whether any item was dropped.
    fn drop_gift_loot_table(&self, loot_table: LootTableRef) -> bool {
        let position = self.position();
        let mut rng = rand::rng();
        // Vanilla `LootContextParamSets.GIFT` carries only ORIGIN and THIS_ENTITY.
        let mut context = LootContext::new(&mut rng)
            .with_origin(position.x, position.y, position.z)
            .with_this_entity(entity_loot_ref(self));

        let items = loot_table.get_random_items(&mut context);
        let dropped_any = !items.is_empty();
        for item in items {
            self.spawn_at_location(item, EGG_LAY_SPAWN_Y_OFFSET);
        }
        dropped_any
    }
}

impl Entity for ChickenEntity {
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
            CHICKEN_BABY_DIMENSIONS.scale(scale)
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

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.current_sound_set().step_sound, 0.15, 1.0);
    }

    fn is_flapping(&self) -> bool {
        let fly_dist = self.base().movement_progress().fly_dist();
        fly_dist > self.chicken_state.lock().next_flap
    }

    fn on_flap(&self) {
        let fly_dist = self.base().movement_progress().fly_dist();
        let mut state = self.chicken_state.lock();
        state.next_flap = fly_dist + state.flap_speed / NEXT_FLAP_SPEED_DIVISOR;
    }

    fn position_rider(&self, passenger: &dyn Entity) {
        position_rider_default(self, passenger);
        if let Some(living) = passenger.as_living_entity() {
            living.set_y_body_rot(self.y_body_rot());
        }
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("IsChickenJockey", i8::from(self.is_chicken_jockey()));
        nbt.insert("EggLayTime", self.egg_time());
        nbt.insert("variant", self.variant().key.to_string());
        nbt.insert("sound_variant", self.sound_variant().key.to_string());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

        self.set_chicken_jockey(nbt.byte("IsChickenJockey").is_some_and(|value| value != 0));
        if let Some(egg_time) = nbt.int("EggLayTime") {
            self.set_egg_time(egg_time);
        }
        if let Some(variant) = nbt.string("variant")
            && let Ok(key) = Identifier::from_str(variant.to_str().as_ref())
        {
            self.set_variant_by_key(&key);
        }
        if let Some(sound_variant) = nbt.string("sound_variant")
            && let Ok(key) = Identifier::from_str(sound_variant.to_str().as_ref())
        {
            self.set_sound_variant_by_key(&key);
        }
    }
}

impl LivingEntity for ChickenEntity {
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

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(self.current_sound_set().hurt_sound)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(self.current_sound_set().death_sound)
    }

    fn chicken_loot_variant(&self) -> Option<&Identifier> {
        Some(&self.variant().key)
    }

    fn base_experience_reward(&self) -> i32 {
        if self.is_chicken_jockey() {
            CHICKEN_JOCKEY_EXPERIENCE_REWARD
        } else {
            Animal::base_experience_reward_animal(self)
        }
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        self.tick_flapping();
        self.tick_egg_laying();
        result
    }
}

impl AgeableMob for ChickenEntity {
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

impl Animal for ChickenEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        ChickenEntity::is_food(item_stack)
    }

    fn breed_variant_key(&self) -> Option<&Identifier> {
        Some(&self.variant().key)
    }

    fn set_breed_variant_key(&self, key: &Identifier) -> bool {
        self.set_variant_by_key(key)
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, offspring: &dyn Animal) {
        let use_self_variant = rand::random::<bool>();
        let variant_key = if use_self_variant {
            self.breed_variant_key()
        } else {
            partner.breed_variant_key()
        };
        let Some(variant_key) = variant_key else {
            return;
        };

        if !offspring.set_breed_variant_key(variant_key) {
            log::error!("chicken offspring could not inherit breeding variant {variant_key}");
        }
    }
}

impl Mob for ChickenEntity {
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
        Some(self.current_sound_set().ambient_sound)
    }

    fn remove_when_far_away(&self, _dist_sqr: f64) -> bool {
        // Chicken jockeys persist like their rider (vanilla `Chicken.removeWhenFarAway`).
        self.is_chicken_jockey()
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let biome = world.biome_at(self.block_position());
        // Divergence: vanilla draws both rolls from the level's shared RNG
        // stream (`level.getRandom()`). Steel has no world RNG stream yet, so
        // both draws use a fresh seed; the selection probabilities are
        // identical. Mirrors the cow/pig convention.
        let (variant, sound_variant) = {
            let mut random = LegacyRandom::from_seed(rand::random());
            let variant = biome.and_then(|biome| {
                REGISTRY
                    .chicken_variants
                    .select_spawn_variant(biome, &mut random)
            });
            let sound_variant = REGISTRY.chicken_sound_variants.pick_random(&mut random);
            (variant, sound_variant)
        };

        if let Some(variant) = variant {
            self.set_variant(variant);
        }

        if let Some(sound_variant) = sound_variant {
            self.set_sound_variant(sound_variant);
        }

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

impl PathfinderMob for ChickenEntity {}

#[cfg(test)]
mod tests;
