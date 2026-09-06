//! Vanilla Fox entity with red/snow variant, behaviour flags, and trusted players.

mod goals;

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{CTakeItemEntity, SoundSource};
use steel_registry::data_components::vanilla_components::{CONSUMABLE, FOOD};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef, MobCategory,
};
use steel_registry::entity_variant::FoxVariant;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_registry::vanilla_entity_data::FoxEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_entities, vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{GameType, InteractionHand};
use steel_utils::{ChunkPos, Downcast as _, DowncastType, DowncastTypeKey, UuidExt};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, ClimbOnTopOfPowderSnowGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal,
    PanicGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, RemovalReason, SpawnGroupData, next_entity_id,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;
use goals::{FoxSearchForItemsGoal, FoxSeekShelterGoal, FoxSleepGoal, PerchAndSearchGoal};

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

/// Pickup delay, in ticks, on an item a fox spits out (vanilla `Fox.spitOutItem`).
const FOX_SPIT_PICKUP_DELAY: i32 = 40;
/// Height above the fox, in blocks, a spat-out item spawns (vanilla `getY() + 1.0`).
const FOX_SPIT_SPAWN_HEIGHT: f64 = 1.0;

/// Chance, per ambient-sound roll at night with nobody near, of the fox screech.
const FOX_SCREECH_CHANCE: f32 = 0.1;
/// Range, in blocks, within which a player suppresses the fox screech.
const FOX_SCREECH_PLAYER_RANGE: f64 = 16.0;
/// Volume the fox screech plays at (vanilla plays it louder than other sounds).
const FOX_SCREECH_VOLUME: f32 = 2.0;

/// Horizontal reach of the fox alert scan (vanilla `alertable` inflates the
/// bounding box by this on X and Z and uses it as the targeting range).
const FOX_ALERT_RANGE: f64 = 12.0;
/// Vertical reach of the fox alert scan (vanilla inflates the box by this on Y).
const FOX_ALERT_VERTICAL_RANGE: f64 = 6.0;

/// Chance a naturally spawned fox holds an item (vanilla `populateDefaultEquipmentSlots`).
const FOX_SPAWN_HELD_ITEM_CHANCE: f32 = 0.2;
// Cumulative weights of the vanilla spawn held-item roll.
const FOX_HELD_EMERALD_ODDS: f32 = 0.05;
const FOX_HELD_EGG_ODDS: f32 = 0.2;
const FOX_HELD_RABBIT_ODDS: f32 = 0.4;
const FOX_HELD_WHEAT_ODDS: f32 = 0.6;
const FOX_HELD_LEATHER_ODDS: f32 = 0.8;

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
    /// Ticks since the fox last ate the food in its mouth (not synced or saved).
    ticks_since_eaten: SyncMutex<i32>,
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
            // Fox goals at their vanilla priorities. The stock float, panic, breed,
            // follow-parent, and look-at-player goals stand in for the fox-specific
            // variants, which differ only in threat- and trust-driven behaviour.
            //
            // The goals vanilla registers that Steel cannot support yet are listed as
            // TODOs at the priority they belong at, each blocked on a foundation that
            // is not in the tree today (a missing mob, a control hook, or the held-item
            // eating path).
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(0, ClimbOnTopOfPowderSnowGoal::new());
            // TODO(fox-goals): 1 FaceplantGoal (needs faceplant physics via a custom FoxMoveControl)
            goal_selector.add_goal(2, PanicGoal::new(2.2));
            goal_selector.add_goal(3, BreedGoal::new(1.0));
            // TODO(fox-goals): 4 AvoidEntityGoal<Player> (needs the trust/defend gate)
            // TODO(fox-goals): 4 AvoidEntityGoal<Wolf> (needs the Wolf mob)
            // TODO(fox-goals): 4 AvoidEntityGoal<PolarBear> (needs the PolarBear mob)
            // TODO(fox-goals): 5 StalkPreyGoal (needs prey mobs and the pounce move control)
            // TODO(fox-goals): 6 FoxPounceGoal (needs pounce/jump physics)
            goal_selector.add_goal(6, FoxSeekShelterGoal::new(1.25));
            // TODO(fox-goals): 7 FoxMeleeAttackGoal (needs an attack target)
            goal_selector.add_goal(7, FoxSleepGoal::new());
            goal_selector.add_goal(8, FollowParentGoal::new(1.25));
            // TODO(fox-goals): 9 StrollThroughVillageGoal (needs village POI)
            // TODO(fox-goals): 10 FoxEatBerriesGoal (deferred with the held-item eating path)
            // TODO(fox-goals): 10 LeapAtTargetGoal (needs an attack target)
            goal_selector.add_goal(11, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(11, FoxSearchForItemsGoal);
            goal_selector.add_goal(12, LookAtPlayerGoal::new(24.0));
            goal_selector.add_goal(13, PerchAndSearchGoal::new());

            // Target-selector goals, none registered yet:
            // TODO(fox-goals): target 3 DefendTrustedTargetGoal (needs the trust/defend gate)
            // TODO(fox-goals): target NearestAttackableTarget for chickens/rabbits (needs the Rabbit mob)
            // TODO(fox-goals): target NearestAttackableTarget for baby turtles on land (needs the Turtle entity, #490)
            // TODO(fox-goals): target NearestAttackableTarget for schooling fish (needs the fish mobs)
        }

        let fox = Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
            ticks_since_eaten: SyncMutex::new(0),
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

    /// Vanilla `Fox.clearStates`: clears the interested, crouching, sitting,
    /// sleeping, defending, and faceplanted flags.
    pub(crate) fn clear_states(&self) {
        self.set_interested(false);
        self.set_crouching(false);
        self.set_sitting(false);
        self.set_sleeping(false);
        self.set_defending(false);
        self.set_faceplanted(false);
    }

    /// Returns vanilla `Fox.canMove`: not sleeping, sitting, or faceplanted.
    pub(crate) fn can_move(&self) -> bool {
        !self.is_sleeping() && !self.is_sitting() && !self.is_faceplanted()
    }

    /// Returns whether this fox trusts the given entity uuid (vanilla `Fox.trusts`).
    #[must_use]
    pub fn trusts(&self, uuid: Uuid) -> bool {
        let entity_data = self.entity_data.lock();
        *entity_data.trusted_id_0.get() == Some(uuid)
            || *entity_data.trusted_id_1.get() == Some(uuid)
    }

    /// Returns vanilla `Fox.FoxBehaviorGoal.alertable`: whether a nearby entity
    /// the fox treats as a threat or prey is within alert range. A resting or
    /// perching fox uses this to stay wary. Mirrors vanilla's combat targeting
    /// (range, no line-of-sight requirement) plus `FoxAlertableEntitiesSelector`.
    pub(crate) fn is_alertable(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let trusted = self.trusted_ids();
        let alertable_targeting = TargetingConditions::for_combat()
            .range(FOX_ALERT_RANGE)
            .ignore_line_of_sight()
            .selector(move |target, _world| fox_alertable_selector(target, &trusted));
        let search_box = self.bounding_box().inflate_xyz(
            FOX_ALERT_RANGE,
            FOX_ALERT_VERTICAL_RANGE,
            FOX_ALERT_RANGE,
        );
        world.has_entity_in_aabb_matching(&search_box, |entity| {
            entity.as_living_entity().is_some_and(|living| {
                alertable_targeting.test(world.as_ref(), Some(self as &dyn LivingEntity), living)
            })
        })
    }

    /// Adds a trusted entity uuid, filling the first free of the two trusted slots.
    pub fn add_trusted(&self, uuid: Uuid) {
        let mut entity_data = self.entity_data.lock();
        if entity_data.trusted_id_0.get().is_none() {
            entity_data.trusted_id_0.set(Some(uuid));
        } else {
            entity_data.trusted_id_1.set(Some(uuid));
        }
    }

    /// Returns the uuids this fox trusts (the filled trusted slots).
    fn trusted_ids(&self) -> Vec<Uuid> {
        let entity_data = self.entity_data.lock();
        [
            *entity_data.trusted_id_0.get(),
            *entity_data.trusted_id_1.get(),
        ]
        .into_iter()
        .flatten()
        .collect()
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

    /// Returns vanilla `Fox.isConsumableFood`: an item the fox can eat from its mouth.
    fn is_consumable_food(item_stack: &ItemStack) -> bool {
        item_stack.has(FOOD) && item_stack.has(CONSUMABLE)
    }

    /// Vanilla `Fox.spitOutItem`: throw an item out just ahead of the fox's head.
    fn spit_out_item(&self, world: &Arc<World>, item_stack: ItemStack) {
        if item_stack.is_empty() {
            return;
        }

        let look = self.look_angle();
        let position = self.position();
        let spawn = DVec3::new(
            position.x + look.x,
            position.y + FOX_SPIT_SPAWN_HEIGHT,
            position.z + look.z,
        );
        let item = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            next_entity_id(),
            spawn,
            item_stack,
            Arc::downgrade(world),
        );
        item.set_pickup_delay(FOX_SPIT_PICKUP_DELAY);
        item.set_thrower(self.uuid());
        self.play_sound(&sound_events::ENTITY_FOX_SPIT, 1.0, 1.0);
        let _ = world.try_add_entity(Arc::new(item));
    }

    /// Vanilla `Fox.dropItemStack`: drop an item at the fox's feet.
    fn drop_item_stack(&self, world: &Arc<World>, item_stack: ItemStack) {
        if item_stack.is_empty() {
            return;
        }

        let item = ItemEntity::with_item(
            &vanilla_entities::ITEM,
            next_entity_id(),
            self.position(),
            item_stack,
            Arc::downgrade(world),
        );
        let _ = world.try_add_entity(Arc::new(item));
    }

    /// Returns whether no player is close enough to suppress the fox screech.
    fn no_player_within_screech_range(&self, world: &Arc<World>) -> bool {
        let search = self.bounding_box().inflate(FOX_SCREECH_PLAYER_RANGE);
        world
            .get_entities_in_aabb_matching(&search, |entity| {
                // Vanilla `EntitySelector.NO_SPECTATORS`: a spectator does not count.
                entity.entity_type() == &vanilla_entities::PLAYER && !entity.is_spectator()
            })
            .is_empty()
    }

    /// Rolls the vanilla `populateDefaultEquipmentSlots` item a fox spawns holding.
    fn spawn_held_item() -> ItemStack {
        let odds = rand::random::<f32>();
        let item = if odds < FOX_HELD_EMERALD_ODDS {
            &vanilla_items::EMERALD
        } else if odds < FOX_HELD_EGG_ODDS {
            &vanilla_items::EGG
        } else if odds < FOX_HELD_RABBIT_ODDS {
            if rand::random::<bool>() {
                &vanilla_items::RABBIT_FOOT
            } else {
                &vanilla_items::RABBIT_HIDE
            }
        } else if odds < FOX_HELD_WHEAT_ODDS {
            &vanilla_items::WHEAT
        } else if odds < FOX_HELD_LEATHER_ODDS {
            &vanilla_items::LEATHER
        } else {
            &vanilla_items::FEATHER
        };
        ItemStack::new(item)
    }

    /// Advances the vanilla `Fox.ticksSinceEaten` timer that gates item swapping.
    ///
    /// Only the timer runs for now, so a fox picks up and holds food without eating it.
    // TODO(fox-eating): finish eating held food here (vanilla consumes the item and
    // applies its on-use effects, e.g. a chorus fruit teleports the fox) once Steel
    // has a mob consume path.
    fn advance_feeding_timer(&self) {
        if Entity::is_alive(self) {
            *self.ticks_since_eaten.lock() += 1;
        }
    }
}

/// Vanilla `Fox.FoxAlertableEntitiesSelector`: which nearby entities make a fox
/// wary. Foxes ignore other foxes; react to chickens, rabbits, and monsters;
/// ignore creative or spectating players and anyone they trust; and otherwise
/// react to any entity that is awake and not sneaking.
fn fox_alertable_selector(target: &dyn LivingEntity, trusted: &[Uuid]) -> bool {
    let entity_type = target.entity_type();
    if entity_type == &vanilla_entities::FOX {
        return false;
    }
    if entity_type == &vanilla_entities::CHICKEN
        || entity_type == &vanilla_entities::RABBIT
        || entity_type.mob_category == MobCategory::Monster
    {
        return true;
    }
    // TODO(tamable-animal): vanilla also alerts on an untamed `TamableAnimal`
    // (wolf, cat, parrot) here; none exist in Steel yet, so that branch is omitted.
    if let Some(player) = target.as_player()
        && (player.is_spectator() || player.game_mode() == GameType::Creative)
    {
        return false;
    }
    if trusted.contains(&target.uuid()) {
        return false;
    }
    !target.is_sleeping() && !target.is_discrete()
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

        let trusted = self.trusted_ids();
        if !trusted.is_empty() {
            let ids = trusted
                .iter()
                .map(|uuid| uuid.to_int_array().to_vec())
                .collect();
            nbt.insert("Trusted", NbtTag::List(NbtList::IntArray(ids)));
        }
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
        if let Some(trusted) = nbt.list("Trusted")
            && let Some(ids) = trusted.int_arrays()
        {
            for id in ids {
                if let Some(uuid) = Uuid::from_int_array(&id.to_vec()) {
                    self.add_trusted(uuid);
                }
            }
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

    fn drop_custom_death_equipment(&self, world: &Arc<World>) {
        // Vanilla Fox.dropAllDeathLoot: spit the held mouth item out on death,
        // before the loot rules, so it drops even for a baby or with mob loot off.
        let held = self
            .living_base()
            .equipment()
            .lock()
            .take(EquipmentSlot::MainHand);
        if !held.is_empty() {
            self.drop_item_stack(world, held);
        }
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        self.advance_feeding_timer();
        let result = Mob::mob_ai_step(self);

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

    fn play_eating_sound(&self) {
        // Vanilla Fox plays ENTITY_FOX_EAT when it eats; the Animal feed path
        // calls this hook for an adult or a growing kit.
        self.play_sound(&sound_events::ENTITY_FOX_EAT, 1.0, 1.0);
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
        let Some(offspring) = offspring.downcast_ref::<FoxEntity>() else {
            return;
        };
        offspring.set_variant(variant);

        // Vanilla FoxBreedGoal.breed: the kit trusts each parent's love-cause
        // player, skipping the partner's when both were bred by the same player.
        let own_cause = self.love_cause_uuid();
        if let Some(own_cause) = own_cause {
            offspring.add_trusted(own_cause);
        }
        if let Some(partner_cause) = partner.love_cause_uuid()
            && own_cause != Some(partner_cause)
        {
            offspring.add_trusted(partner_cause);
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
            return Some(&sound_events::ENTITY_FOX_SLEEP);
        }
        // A fox occasionally screeches at night when no player is watching.
        if let Some(world) = self.level()
            && !world.is_bright_outside()
            && rand::random::<f32>() < FOX_SCREECH_CHANCE
            && self.no_player_within_screech_range(&world)
        {
            return Some(&sound_events::ENTITY_FOX_SCREECH);
        }
        Some(&sound_events::ENTITY_FOX_AMBIENT)
    }

    fn play_ambient_sound(&self) {
        // Vanilla Fox.playAmbientSound plays the screech louder than other sounds.
        let ambient = self.ambient_sound();
        if ambient.is_some_and(|sound| sound.key == sound_events::ENTITY_FOX_SCREECH.key) {
            self.play_sound(
                &sound_events::ENTITY_FOX_SCREECH,
                FOX_SCREECH_VOLUME,
                self.voice_pitch(),
            );
        } else {
            self.make_sound(ambient);
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

        // Vanilla `populateDefaultEquipmentSlots`: a fifth of foxes spawn holding an item.
        // (Vanilla shares the variant across a spawn group; picking it from the biome per
        // fox gives the same uniform result, since a group shares one spawn biome.)
        if rand::random::<f32>() < FOX_SPAWN_HELD_ITEM_CHANCE {
            self.living_base()
                .equipment()
                .lock()
                .set(EquipmentSlot::MainHand, Self::spawn_held_item());
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

    /// Vanilla `Fox.canHoldItem`: a fox holds an item if its mouth is empty, or it
    /// will swap a non-food item already held for a food item.
    fn can_hold_item(&self, item_stack: &ItemStack) -> bool {
        let equipment = self.living_base().equipment().lock();
        let held = equipment.get_ref(EquipmentSlot::MainHand);
        held.is_empty()
            || (*self.ticks_since_eaten.lock() > 0
                && Self::is_consumable_food(item_stack)
                && !Self::is_consumable_food(held))
    }

    /// Vanilla `Fox.pickUpItem`: hold one of the item in the mouth, spitting out
    /// whatever was there and dropping any extra count.
    fn pick_up_item(&self, world: &Arc<World>, item_entity: &ItemEntity) {
        let mut item_stack = item_entity.get_item();
        if !self.can_hold_item(&item_stack) {
            return;
        }

        let count = item_stack.count();
        if count > 1 {
            self.drop_item_stack(world, item_stack.split(count - 1));
        }

        let held = self
            .living_base()
            .equipment()
            .lock()
            .take(EquipmentSlot::MainHand);
        self.spit_out_item(world, held);

        let one = item_stack.split(1);
        self.living_base()
            .equipment()
            .lock()
            .set(EquipmentSlot::MainHand, one);
        self.set_guaranteed_drop(EquipmentSlot::MainHand);

        let chunk_pos = ChunkPos::from_entity_pos(item_entity.position());
        world.broadcast_to_nearby(
            chunk_pos,
            CTakeItemEntity::new(item_entity.id(), self.id(), 1),
            None,
        );
        item_entity.set_removed(RemovalReason::Discarded);
        *self.ticks_since_eaten.lock() = 0;
    }
}

impl PathfinderMob for FoxEntity {}

#[cfg(test)]
mod tests;
