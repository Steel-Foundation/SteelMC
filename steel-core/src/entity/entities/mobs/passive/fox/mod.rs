//! Vanilla Fox entity.

mod goals;

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{CTakeItemEntity, SoundSource};
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::vanilla_components::{CONSUMABLE, FOOD};
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef, MobCategory,
};
use steel_registry::entity_variant::FoxVariant;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::FoxEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, level_events, sound_events, vanilla_attributes, vanilla_entities,
    vanilla_items,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::types::{GameType, InteractionHand};
use steel_utils::{BlockPos, ChunkPos, Downcast as _, DowncastType, DowncastTypeKey, UuidExt};
use uuid::Uuid;

use crate::behavior::{ITEM_BEHAVIORS, InteractionResult};
use crate::entity::ai::goal::{
    ClimbOnTopOfPowderSnowGoal, LeapAtTargetGoal, NearestAttackableTargetGoal,
    WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::targeting::TargetingConditions;
use crate::entity::damage::DamageSource;
use crate::entity::entities::objects::items::ItemEntity;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, RemovalReason, SharedEntity, SpawnGroupData, next_entity_id,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::{LevelReader, World};
use goals::{
    DefendTrustedTargetGoal, FoxBreedGoal, FoxFloatGoal, FoxFollowParentGoal, FoxLookAtPlayerGoal,
    FoxMeleeAttackGoal, FoxPanicGoal, FoxPounceGoal, FoxSearchForItemsGoal, FoxSleepGoal,
    PerchAndSearchGoal, StalkPreyGoal,
};

const FACEPLANT_PARTICLE_CHANCE: f32 = 0.2;
const LEAP_AT_TARGET_HEIGHT: f32 = 0.4;
const BABY_SCALE: f32 = 0.6;
const FOX_BABY_WIDTH: f32 = 0.6 * BABY_SCALE;
const FOX_BABY_HEIGHT: f32 = 0.7 * BABY_SCALE;
const FOX_BABY_EYE_HEIGHT: f32 = 0.343_75;
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

const FLAG_SITTING: i8 = 1;
const FLAG_CROUCHING: i8 = 1 << 2;
const FLAG_INTERESTED: i8 = 1 << 3;
const FLAG_POUNCING: i8 = 1 << 4;
const FLAG_SLEEPING: i8 = 1 << 5;
const FLAG_FACEPLANTED: i8 = 1 << 6;
const FLAG_DEFENDING: i8 = 1 << 7;

const FOX_SPIT_PICKUP_DELAY: i32 = 40;
const FOX_SPIT_SPAWN_HEIGHT: f64 = 1.0;

const FOX_EAT_TICKS: i32 = 600;
const FOX_CHEW_TICKS: i32 = 560;
const FOX_CHEW_SOUND_CHANCE: f32 = 0.1;

const FOX_SCREECH_CHANCE: f32 = 0.1;
const FOX_SCREECH_PLAYER_RANGE: f64 = 16.0;
const FOX_SCREECH_VOLUME: f32 = 2.0;

const FOX_ALERT_RANGE: f64 = 12.0;
const FOX_ALERT_VERTICAL_RANGE: f64 = 6.0;

const CROUCH_STEP: f32 = 0.2;
const FULLY_CROUCHED: f32 = 5.0;

const FOX_PREY_TARGET_INTERVAL: i32 = 10;

const FOX_SPAWN_HELD_ITEM_CHANCE: f32 = 0.2;
const FOX_HELD_EMERALD_ODDS: f32 = 0.05;
const FOX_HELD_EGG_ODDS: f32 = 0.2;
const FOX_HELD_RABBIT_ODDS: f32 = 0.4;
const FOX_HELD_WHEAT_ODDS: f32 = 0.6;
const FOX_HELD_LEATHER_ODDS: f32 = 0.8;

#[entity_behavior(class = "Fox")]
/// Vanilla fox entity.
pub struct FoxEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<FoxEntityData>,
    ticks_since_eaten: SyncMutex<i32>,
    crouch_amount: SyncMutex<f32>,
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
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FoxFloatGoal::new(&mob_base));
            goal_selector.add_goal(0, ClimbOnTopOfPowderSnowGoal::new());
            // TODO(fox-goals): 1 FaceplantGoal (needs faceplant physics via a custom FoxMoveControl)
            goal_selector.add_goal(2, FoxPanicGoal::new(2.2));
            goal_selector.add_goal(3, FoxBreedGoal::new(1.0));
            // TODO(fox-goals): 4 AvoidEntityGoal<Player> (needs the trust/defend gate)
            // TODO(fox-goals): 4 AvoidEntityGoal<Wolf> (needs the Wolf mob)
            // TODO(fox-goals): 4 AvoidEntityGoal<PolarBear> (needs the PolarBear mob)
            goal_selector.add_goal(5, StalkPreyGoal);
            goal_selector.add_goal(6, FoxPounceGoal);
            // TODO(fox-goals): 6 SeekShelterGoal (needs a FleeSunGoal move target)
            goal_selector.add_goal(7, FoxMeleeAttackGoal::new(1.2));
            goal_selector.add_goal(7, FoxSleepGoal::new());
            goal_selector.add_goal(8, FoxFollowParentGoal::new(1.25));
            // TODO(fox-goals): 9 StrollThroughVillageGoal (needs village POI)
            // TODO(fox-goals): 10 FoxEatBerriesGoal (needs berry picking off a sweet
            // berry bush and off cave vines)
            goal_selector.add_goal(10, LeapAtTargetGoal::new(LEAP_AT_TARGET_HEIGHT));
            goal_selector.add_goal(11, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(11, FoxSearchForItemsGoal);
            goal_selector.add_goal(12, FoxLookAtPlayerGoal::new(24.0));
            goal_selector.add_goal(13, PerchAndSearchGoal::new());

            let mut target_selector = mob_base.target_selector().lock();
            target_selector.add_goal(3, DefendTrustedTargetGoal::new());
            target_selector.add_goal(4, target_stalkable_prey());
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
            crouch_amount: SyncMutex::new(0.0),
        };
        fox.set_can_pick_up_loot(true);
        fox
    }

    pub(crate) fn set_variant(&self, variant: FoxVariant) {
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

    #[must_use]
    pub(crate) fn is_sitting(&self) -> bool {
        self.get_flag(FLAG_SITTING)
    }

    pub(crate) fn set_sitting(&self, sitting: bool) {
        self.set_flag(FLAG_SITTING, sitting);
    }

    #[must_use]
    pub(crate) fn is_crouching(&self) -> bool {
        self.get_flag(FLAG_CROUCHING)
    }

    pub(crate) fn set_crouching(&self, crouching: bool) {
        self.set_flag(FLAG_CROUCHING, crouching);
    }

    #[must_use]
    pub(crate) fn is_interested(&self) -> bool {
        self.get_flag(FLAG_INTERESTED)
    }

    pub(crate) fn set_interested(&self, interested: bool) {
        self.set_flag(FLAG_INTERESTED, interested);
    }

    #[must_use]
    pub(crate) fn is_pouncing(&self) -> bool {
        self.get_flag(FLAG_POUNCING)
    }

    pub(crate) fn set_pouncing(&self, pouncing: bool) {
        self.set_flag(FLAG_POUNCING, pouncing);
    }

    #[must_use]
    pub(crate) fn is_sleeping(&self) -> bool {
        self.get_flag(FLAG_SLEEPING)
    }

    pub(crate) fn set_sleeping(&self, sleeping: bool) {
        self.set_flag(FLAG_SLEEPING, sleeping);
    }

    #[must_use]
    pub(crate) fn is_faceplanted(&self) -> bool {
        self.get_flag(FLAG_FACEPLANTED)
    }

    pub(crate) fn set_faceplanted(&self, faceplanted: bool) {
        self.set_flag(FLAG_FACEPLANTED, faceplanted);
    }

    #[must_use]
    pub(crate) fn is_defending(&self) -> bool {
        self.get_flag(FLAG_DEFENDING)
    }

    pub(crate) fn set_defending(&self, defending: bool) {
        self.set_flag(FLAG_DEFENDING, defending);
    }

    pub(crate) fn is_fully_crouched(&self) -> bool {
        *self.crouch_amount.lock() >= FULLY_CROUCHED
    }

    pub(crate) fn reset_crouch_amount(&self) {
        *self.crouch_amount.lock() = 0.0;
    }

    fn tick_pounce_state(&self) {
        let target_alive = Mob::target(self).is_some_and(|target| target.is_alive());
        if !target_alive {
            self.set_crouching(false);
            self.set_interested(false);
        }

        let mut crouch = self.crouch_amount.lock();
        if self.is_crouching() {
            *crouch = (*crouch + CROUCH_STEP).min(FULLY_CROUCHED);
        } else {
            *crouch = 0.0;
        }
    }

    pub(crate) fn clear_states(&self) {
        self.set_interested(false);
        self.set_crouching(false);
        self.set_sitting(false);
        self.set_sleeping(false);
        self.set_defending(false);
        self.set_faceplanted(false);
    }

    pub(crate) fn can_move(&self) -> bool {
        !self.is_sleeping() && !self.is_sitting() && !self.is_faceplanted()
    }

    #[must_use]
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the avoid-player goal that reads this lands separately"
        )
    )]
    pub(crate) fn trusts(&self, uuid: Uuid) -> bool {
        let entity_data = self.entity_data.lock();
        *entity_data.trusted_id_0.get() == Some(uuid)
            || *entity_data.trusted_id_1.get() == Some(uuid)
    }

    /// Whether a threat or prey is within alert range.
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

    /// Adds a trusted entity uuid, filling the first free trusted slot.
    pub fn add_trusted(&self, uuid: Uuid) {
        let mut entity_data = self.entity_data.lock();
        if entity_data.trusted_id_0.get().is_none() {
            entity_data.trusted_id_0.set(Some(uuid));
        } else {
            entity_data.trusted_id_1.set(Some(uuid));
        }
    }

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

    #[must_use]
    /// Whether an item stack is fox food.
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::FOX_FOOD)
    }

    fn is_consumable_food(item_stack: &ItemStack) -> bool {
        item_stack.has(FOOD) && item_stack.has(CONSUMABLE)
    }

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

    fn no_player_within_screech_range(&self, world: &Arc<World>) -> bool {
        let search = self.bounding_box().inflate(FOX_SCREECH_PLAYER_RANGE);
        world
            .get_entities_in_aabb_matching(&search, |entity| {
                entity.entity_type() == &vanilla_entities::PLAYER && !entity.is_spectator()
            })
            .is_empty()
    }

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

    fn can_eat(&self) -> bool {
        let mut holds_food = false;
        self.with_equipment_slot(EquipmentSlot::MainHand, &mut |item_stack| {
            holds_food = Self::is_consumable_food(item_stack);
        });
        holds_food && Mob::target(self).is_none() && self.on_ground() && !self.is_sleeping()
    }

    fn tick_fox_posture(&self) {
        if !self.is_effective_ai() {
            return;
        }
        let Some(world) = self.level() else {
            return;
        };

        let in_water = self.is_in_water();
        if in_water || Mob::target(self).is_some() || world.is_thundering() {
            self.set_sleeping(false);
        }

        if in_water || self.is_sleeping() {
            self.set_sitting(false);
        }

        if self.is_faceplanted() && rand::random::<f32>() < FACEPLANT_PARTICLE_CHANCE {
            let pos = self.block_position();
            world.level_event(
                level_events::PARTICLES_DESTROY_BLOCK,
                pos,
                level_events::encode_block_state_data(u32::from(world.get_block_state(pos).0)),
                None,
            );
        }
    }

    fn tick_eating(&self) {
        if !Entity::is_alive(self) || !self.is_effective_ai() {
            return;
        }

        let ticks_since_eaten = {
            let mut ticks_since_eaten = self.ticks_since_eaten.lock();
            *ticks_since_eaten += 1;
            *ticks_since_eaten
        };
        if !self.can_eat() {
            return;
        }

        if ticks_since_eaten > FOX_EAT_TICKS {
            self.swallow_mouth_item();
        } else if ticks_since_eaten > FOX_CHEW_TICKS
            && rand::random::<f32>() < FOX_CHEW_SOUND_CHANCE
        {
            Animal::play_eating_sound(self);
            self.broadcast_entity_event(EntityStatus::FoxEat);
        }
    }

    /// Finishes the mouth item, leaving any container behind.
    fn swallow_mouth_item(&self) {
        let Some(world) = self.level() else {
            return;
        };

        let mut item_in_mouth = self
            .living_base()
            .equipment()
            .lock()
            .take(EquipmentSlot::MainHand);
        let remainder = ITEM_BEHAVIORS
            .get_behavior(item_in_mouth.item())
            .finish_using(&mut item_in_mouth, &world, self);
        self.living_base()
            .equipment()
            .lock()
            .set(EquipmentSlot::MainHand, remainder);

        *self.ticks_since_eaten.lock() = 0;
    }
}

/// React: chicken, rabbit, hostiles. Ignore: fox, creative, spectating, trusted
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

fn target_stalkable_prey() -> NearestAttackableTargetGoal {
    NearestAttackableTargetGoal::new_with_interval(
        FOX_PREY_TARGET_INTERVAL,
        false,
        false,
        |target, _| target.entity_type() == &vanilla_entities::CHICKEN,
    )
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

    fn tick(&self) {
        LivingEntity::tick_living_entity(self);
        self.tick_fox_posture();
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
        self.tick_eating();
        self.tick_pounce_state();
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
        self.play_sound(&sound_events::ENTITY_FOX_EAT, 1.0, 1.0);
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, offspring: &dyn Animal) {
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

    fn check_animal_spawn_rules(
        level: &dyn LevelReader,
        _spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool {
        level
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::FOXES_SPAWNABLE_ON)
            && Self::is_bright_enough_to_spawn(level, pos)
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

    fn set_target(&self, target: Option<&SharedEntity>) -> bool {
        // Vanilla Fox.setTarget: losing the target always drops isDefending, no
        // matter which goal cleared it.
        if self.is_defending() && target.is_none() {
            self.set_defending(false);
        }
        self.mob_base()
            .set_target(target, |target| self.is_valid_target(target))
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        if self.is_sleeping() {
            return Some(&sound_events::ENTITY_FOX_SLEEP);
        }
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

    fn can_hold_item(&self, item_stack: &ItemStack) -> bool {
        let equipment = self.living_base().equipment().lock();
        let held = equipment.get_ref(EquipmentSlot::MainHand);
        held.is_empty()
            || (*self.ticks_since_eaten.lock() > 0
                && Self::is_consumable_food(item_stack)
                && !Self::is_consumable_food(held))
    }

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
