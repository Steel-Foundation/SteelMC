//! Vanilla Fox entity with red/snow variant, behaviour flags, and trusted players.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::entity_variant::FoxVariant;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_registry::vanilla_entity_data::FoxEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{REGISTRY, TaggedRegistryExt, sound_events, vanilla_attributes};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{Downcast as _, DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal,
    WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

/// Baby fox render scale (vanilla `Fox.BABY_SCALE`).
const BABY_SCALE: f32 = 0.6;
const FOX_BABY_WIDTH: f32 = 0.6 * BABY_SCALE;
const FOX_BABY_HEIGHT: f32 = 0.7 * BABY_SCALE;
/// Vanilla baby fox eye height.
const FOX_BABY_EYE_HEIGHT: f32 = 0.343_75;
/// Vanilla baby fox passenger attachment height.
const FOX_BABY_PASSENGER_Y: f64 = 0.375;

const FOX_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, FOX_BABY_PASSENGER_Y, 0.0)];
const FOX_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    FOX_BABY_WIDTH,
    FOX_BABY_HEIGHT,
    FOX_BABY_EYE_HEIGHT,
    EntityAttachments::new(&FOX_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);
const DEFAULT_STEP_HEIGHT: f32 = 0.6;

// Vanilla `Fox.DATA_FLAGS_ID` bit flags (a single synced byte).
const FLAG_SITTING: i8 = 1;
const FLAG_CROUCHING: i8 = 1 << 2;
const FLAG_INTERESTED: i8 = 1 << 3;
const FLAG_POUNCING: i8 = 1 << 4;
const FLAG_SLEEPING: i8 = 1 << 5;
const FLAG_FACEPLANTED: i8 = 1 << 6;
const FLAG_DEFENDING: i8 = 1 << 7;

#[entity_behavior(class = "Fox")]
/// Vanilla fox entity with synced variant, behaviour flags, and trusted players.
pub struct FoxEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<FoxEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `FoxEntity`.
unsafe impl DowncastType for FoxEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/fox");
}

impl FoxEntity {
    /// Creates a new fox at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a fox from persisted base entity state.
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
        let mut entity_data = FoxEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            // INTERIM goal set: enough for a fox that floats, panics, breeds, follows a
            // parent, wanders, and looks around. The full bespoke fox goal suite (stalk,
            // pounce, sleep, seek shelter, eat berries, search for items, avoid threats,
            // defend trusted players) lands in the follow-up goals PR.
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(2, PanicGoal::new(2.2));
            goal_selector.add_goal(3, BreedGoal::new(1.0));
            goal_selector.add_goal(8, FollowParentGoal::new(1.25));
            goal_selector.add_goal(11, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(12, LookAtPlayerGoal::new(24.0));
        }

        let fox = Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
        };
        // Vanilla foxes pick up dropped items (`setCanPickUpLoot(true)`).
        fox.set_can_pick_up_loot(true);
        fox
    }

    /// Sets the fox variant (red or snow).
    pub fn set_variant(&self, variant: FoxVariant) {
        self.entity_data.lock().variant_type.set(variant.id());
    }

    /// Returns the fox variant, defaulting to red for any unknown id.
    #[must_use]
    pub fn variant(&self) -> FoxVariant {
        FoxVariant::by_id(*self.entity_data.lock().variant_type.get())
    }

    fn get_flag(&self, flag: i8) -> bool {
        (*self.entity_data.lock().flags.get() & flag) != 0
    }

    fn set_flag(&self, flag: i8, value: bool) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.flags.get();
        let updated = if value {
            current | flag
        } else {
            current & !flag
        };
        entity_data.flags.set(updated);
    }

    /// Returns vanilla `Fox.isSitting`.
    #[must_use]
    pub fn is_sitting(&self) -> bool {
        self.get_flag(FLAG_SITTING)
    }

    /// Sets vanilla `Fox.setSitting`.
    pub fn set_sitting(&self, sitting: bool) {
        self.set_flag(FLAG_SITTING, sitting);
    }

    /// Returns vanilla `Fox.isCrouching`.
    #[must_use]
    pub fn is_crouching(&self) -> bool {
        self.get_flag(FLAG_CROUCHING)
    }

    /// Sets vanilla `Fox.setIsCrouching`.
    pub fn set_crouching(&self, crouching: bool) {
        self.set_flag(FLAG_CROUCHING, crouching);
    }

    /// Returns vanilla `Fox.isInterested`.
    #[must_use]
    pub fn is_interested(&self) -> bool {
        self.get_flag(FLAG_INTERESTED)
    }

    /// Sets vanilla `Fox.setIsInterested`.
    pub fn set_interested(&self, interested: bool) {
        self.set_flag(FLAG_INTERESTED, interested);
    }

    /// Returns vanilla `Fox.isPouncing`.
    #[must_use]
    pub fn is_pouncing(&self) -> bool {
        self.get_flag(FLAG_POUNCING)
    }

    /// Sets vanilla `Fox.setIsPouncing`.
    pub fn set_pouncing(&self, pouncing: bool) {
        self.set_flag(FLAG_POUNCING, pouncing);
    }

    /// Returns vanilla `Fox.isSleeping`.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.get_flag(FLAG_SLEEPING)
    }

    /// Sets vanilla `Fox.setSleeping`.
    pub fn set_sleeping(&self, sleeping: bool) {
        self.set_flag(FLAG_SLEEPING, sleeping);
    }

    /// Returns vanilla `Fox.isFaceplanted`.
    #[must_use]
    pub fn is_faceplanted(&self) -> bool {
        self.get_flag(FLAG_FACEPLANTED)
    }

    /// Sets vanilla `Fox.setFaceplanted`.
    pub fn set_faceplanted(&self, faceplanted: bool) {
        self.set_flag(FLAG_FACEPLANTED, faceplanted);
    }

    /// Returns vanilla `Fox.isDefending`.
    #[must_use]
    pub fn is_defending(&self) -> bool {
        self.get_flag(FLAG_DEFENDING)
    }

    /// Sets vanilla `Fox.setDefending`.
    pub fn set_defending(&self, defending: bool) {
        self.set_flag(FLAG_DEFENDING, defending);
    }

    /// Returns whether this fox trusts the given entity uuid (vanilla `Fox.trusts`).
    #[must_use]
    pub fn trusts(&self, uuid: uuid::Uuid) -> bool {
        let entity_data = self.entity_data.lock();
        *entity_data.trusted_id_0.get() == Some(uuid)
            || *entity_data.trusted_id_1.get() == Some(uuid)
    }

    /// Adds a trusted entity uuid, filling the first free of the two trusted slots.
    pub fn add_trusted(&self, uuid: uuid::Uuid) {
        let mut entity_data = self.entity_data.lock();
        if entity_data.trusted_id_0.get().is_none() {
            entity_data.trusted_id_0.set(Some(uuid));
        } else {
            entity_data.trusted_id_1.set(Some(uuid));
        }
    }

    fn set_variant_by_name(&self, name: &str) -> bool {
        let Some(variant) = FoxVariant::from_serialized_name(name) else {
            return false;
        };
        self.set_variant(variant);
        true
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

    /// Returns whether an item stack matches the vanilla fox food tag (sweet berries).
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::FOX_FOOD)
    }
}

impl Entity for FoxEntity {
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
            FOX_BABY_DIMENSIONS.scale(scale)
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("Type", self.variant().serialized_name());
        nbt.insert("Sleeping", self.is_sleeping());
        nbt.insert("Sitting", self.is_sitting());
        nbt.insert("Crouching", self.is_crouching());
        // TODO(fox-trust-persistence): persist the "Trusted" uuid list once the
        // trust-building interaction and defend-trusted goal land in the goals PR.
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

        if let Some(variant) = nbt.string("Type") {
            self.set_variant_by_name(variant.to_str().as_ref());
        }
        if let Some(sleeping) = nbt.byte("Sleeping") {
            self.set_sleeping(sleeping != 0);
        }
        if let Some(sitting) = nbt.byte("Sitting") {
            self.set_sitting(sitting != 0);
        }
        if let Some(crouching) = nbt.byte("Crouching") {
            self.set_crouching(crouching != 0);
        }
    }
}

impl LivingEntity for FoxEntity {
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
        Some(&sound_events::ENTITY_FOX_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_FOX_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        result
    }
}

impl AgeableMob for FoxEntity {
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

impl Animal for FoxEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        FoxEntity::is_food(item_stack)
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, offspring: &dyn Animal) {
        // Vanilla: the kit inherits one random parent's variant.
        let variant = if rand::random::<bool>() {
            self.variant()
        } else {
            partner
                .downcast_ref::<FoxEntity>()
                .map_or_else(|| self.variant(), FoxEntity::variant)
        };
        if let Some(offspring) = offspring.downcast_ref::<FoxEntity>() {
            offspring.set_variant(variant);
        }
    }
}

impl Mob for FoxEntity {
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
        if self.is_sleeping() {
            Some(&sound_events::ENTITY_FOX_SLEEP)
        } else {
            // TODO(fox-screech): emit ENTITY_FOX_SCREECH at night when no player is
            // nearby, per vanilla getAmbientSound.
            Some(&sound_events::ENTITY_FOX_AMBIENT)
        }
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let variant = world
            .biome_at(self.block_position())
            .map_or(FoxVariant::Red, |biome| {
                if biome.has_tag(&BiomeTag::SPAWNS_SNOW_FOXES) {
                    FoxVariant::Snow
                } else {
                    FoxVariant::Red
                }
            });
        self.set_variant(variant);
        // TODO(fox-spawn): share the rolled variant across a spawn group and roll the
        // vanilla 20% chance to spawn holding an item, once the goals PR needs them.
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

impl PathfinderMob for FoxEntity {}

#[cfg(test)]
mod tests;
