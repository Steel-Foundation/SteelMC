//! Vanilla Mooshroom (MushroomCow) entity with variant, shearing, and stew-feeding parity.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use simdnbt::{FromNbtTag as _, ToNbtTag as _};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::data_components::components::SuspiciousStewEffects;
use steel_registry::data_components::vanilla_components::SUSPICIOUS_STEW_EFFECTS;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::ParticleData;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_entity_data::MooshroomEntityData;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_blocks,
    vanilla_entities, vanilla_items, vanilla_loot_tables, vanilla_particle_types,
    vanilla_suspicious_stew_effects,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Downcast as _, DowncastType, DowncastTypeKey};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::entities::CowEntity;
use crate::entity::living_entity::shearing_loot_items_with_rng;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, RemovalReason, SpawnGroupData, next_entity_id,
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, World};

const MOOSHROOM_BABY_PASSENGER_Y_OFFSET: f64 = 0.75;
const MOOSHROOM_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(
        0.0,
        MOOSHROOM_BABY_PASSENGER_Y_OFFSET,
        0.0,
    )];
const MOOSHROOM_BABY_WIDTH: f32 = 0.45;
const MOOSHROOM_BABY_HEIGHT: f32 = 0.7;
const MOOSHROOM_BABY_EYE_HEIGHT: f32 = 0.69;

const MOOSHROOM_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    MOOSHROOM_BABY_WIDTH,
    MOOSHROOM_BABY_HEIGHT,
    MOOSHROOM_BABY_EYE_HEIGHT,
    EntityAttachments::new(&MOOSHROOM_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);
const DEFAULT_STEP_HEIGHT: f32 = 0.6;
/// Vanilla 1 in 1024 chance of mutation when breeding identical variants.
const MUTATE_CHANCE: u32 = 1024;
const MOOSHROOM_WALK_TARGET_VALUE: f32 = 10.0;
const SHEARING_DROP_HEIGHT_OFFSET: f64 = 1.0;
const SHEARING_DROP_HORIZONTAL_JITTER: f64 = 0.1;
const SHEARING_DROP_VERTICAL_JITTER: f64 = 0.05;
const MOOSHROOM_CONVERSION_PARTICLE_COUNT: i32 = 1;
const MOOSHROOM_CONVERSION_PARTICLE_HEIGHT_OFFSET: f64 = 0.5;
const MOOSHROOM_CONVERSION_PARTICLE_SPREAD: DVec3 = DVec3::ZERO;
const MOOSHROOM_CONVERSION_PARTICLE_SPEED: f64 = 0.0;
const MOOSHROOM_RED_VARIANT_ID: i32 = 0;
const MOOSHROOM_BROWN_VARIANT_ID: i32 = 1;
const MOOSHROOM_FLOAT_GOAL_PRIORITY: i32 = 0;
const MOOSHROOM_PANIC_GOAL_PRIORITY: i32 = 1;
const MOOSHROOM_BREED_GOAL_PRIORITY: i32 = 2;
const MOOSHROOM_TEMPT_GOAL_PRIORITY: i32 = 3;
const MOOSHROOM_FOLLOW_PARENT_GOAL_PRIORITY: i32 = 4;
const MOOSHROOM_STROLL_GOAL_PRIORITY: i32 = 5;
const MOOSHROOM_LOOK_AT_PLAYER_GOAL_PRIORITY: i32 = 6;
const MOOSHROOM_RANDOM_LOOK_GOAL_PRIORITY: i32 = 7;
const MOOSHROOM_PANIC_SPEED: f64 = 2.0;
const MOOSHROOM_BREED_SPEED: f64 = 1.0;
const MOOSHROOM_TEMPT_SPEED: f64 = 1.25;
const MOOSHROOM_FOLLOW_PARENT_SPEED: f64 = 1.25;
const MOOSHROOM_STROLL_SPEED: f64 = 1.0;
const MOOSHROOM_LOOK_AT_PLAYER_RANGE: f64 = 6.0;
const MOOSHROOM_CONVERT_SOUND_VOLUME: f32 = 2.0;
const MOOSHROOM_EAT_SOUND_VOLUME: f32 = 2.0;
const DEFAULT_SOUND_VOLUME: f32 = 1.0;
const DEFAULT_SOUND_PITCH: f32 = 1.0;
const COW_STEP_SOUND_VOLUME: f32 = 0.15;
const MOOSHROOM_SOUND_VOLUME: f32 = 0.4;
const MIN_HEALTH: f32 = 0.0;

/// Vanilla Mooshroom variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MushroomCowVariant {
    /// Red Mooshroom variant.
    #[default]
    Red,
    /// Brown Mooshroom variant.
    Brown,
}

impl MushroomCowVariant {
    /// Returns the serialized variant identifier name.
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Brown => "brown",
        }
    }

    /// Parses a variant from its serialized identifier name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "red" => Some(Self::Red),
            "brown" => Some(Self::Brown),
            _ => None,
        }
    }

    /// Returns the vanilla integer ID of this variant.
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Red => MOOSHROOM_RED_VARIANT_ID,
            Self::Brown => MOOSHROOM_BROWN_VARIANT_ID,
        }
    }

    /// Resolves a variant from its vanilla integer ID.
    #[must_use]
    pub const fn from_id(id: i32) -> Self {
        match id {
            MOOSHROOM_BROWN_VARIANT_ID => Self::Brown,
            _ => Self::Red,
        }
    }

    /// Returns the opposite variant (`Red` <-> `Brown`).
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Red => Self::Brown,
            Self::Brown => Self::Red,
        }
    }
}

/// Vanilla Mooshroom entity.
#[entity_behavior(class = "MushroomCow")]
pub struct MushroomCowEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<MooshroomEntityData>,
    stew_effects: SyncMutex<Option<SuspiciousStewEffects>>,
    last_lightning_bolt_uuid: SyncMutex<Option<Uuid>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `MushroomCowEntity`.
unsafe impl DowncastType for MushroomCowEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/mooshroom");
}

impl MushroomCowEntity {
    /// Creates a new mooshroom at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs a mooshroom from persisted base entity state.
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
        let mut entity_data = MooshroomEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(MOOSHROOM_FLOAT_GOAL_PRIORITY, FloatGoal::new(&mob_base));
            goal_selector.add_goal(
                MOOSHROOM_PANIC_GOAL_PRIORITY,
                PanicGoal::new(MOOSHROOM_PANIC_SPEED),
            );
            goal_selector.add_goal(
                MOOSHROOM_BREED_GOAL_PRIORITY,
                BreedGoal::new(MOOSHROOM_BREED_SPEED),
            );
            goal_selector.add_goal(
                MOOSHROOM_TEMPT_GOAL_PRIORITY,
                TemptGoal::new(
                    MOOSHROOM_TEMPT_SPEED,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::COW_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(
                MOOSHROOM_FOLLOW_PARENT_GOAL_PRIORITY,
                FollowParentGoal::new(MOOSHROOM_FOLLOW_PARENT_SPEED),
            );
            goal_selector.add_goal(
                MOOSHROOM_STROLL_GOAL_PRIORITY,
                WaterAvoidingRandomStrollGoal::new(MOOSHROOM_STROLL_SPEED),
            );
            goal_selector.add_goal(
                MOOSHROOM_LOOK_AT_PLAYER_GOAL_PRIORITY,
                LookAtPlayerGoal::new(MOOSHROOM_LOOK_AT_PLAYER_RANGE),
            );
            goal_selector.add_goal(
                MOOSHROOM_RANDOM_LOOK_GOAL_PRIORITY,
                RandomLookAroundGoal::new(),
            );
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
            stew_effects: SyncMutex::new(None),
            last_lightning_bolt_uuid: SyncMutex::new(None),
        }
    }

    /// Sets the active mooshroom variant.
    pub fn set_variant(&self, variant: MushroomCowVariant) {
        self.entity_data
            .lock()
            .mushroom_cow_mut()
            .variant_type
            .set(variant.id());
    }

    /// Returns the active mooshroom variant.
    #[must_use]
    pub fn variant(&self) -> MushroomCowVariant {
        let binding = self.entity_data.lock();
        let id = *binding.mushroom_cow().variant_type.get();
        MushroomCowVariant::from_id(id)
    }

    /// Sets the pending suspicious stew effects from feeding a flower.
    pub fn set_stew_effects(&self, effects: Option<SuspiciousStewEffects>) {
        *self.stew_effects.lock() = effects;
    }

    /// Returns the pending suspicious stew effects, if any.
    #[must_use]
    pub fn stew_effects(&self) -> Option<SuspiciousStewEffects> {
        self.stew_effects.lock().clone()
    }

    /// Handles a lightning strike hitting the mooshroom.
    pub fn thunder_hit(&self, lightning_uuid: Uuid) {
        // TODO: Route server-side LightningBolt hits here once that shared entity and dispatch exist.
        let mut last_uuid = self.last_lightning_bolt_uuid.lock();
        if last_uuid.as_ref() == Some(&lightning_uuid) {
            return;
        }
        *last_uuid = Some(lightning_uuid);
        self.set_variant(self.variant().opposite());
        self.play_sound(
            &sound_events::ENTITY_MOOSHROOM_CONVERT,
            MOOSHROOM_CONVERT_SOUND_VOLUME,
            DEFAULT_SOUND_PITCH,
        );
    }

    /// Returns whether the mooshroom can currently be sheared.
    #[must_use]
    pub fn ready_for_shearing(&self) -> bool {
        !AgeableMob::is_baby(self)
    }

    /// Shears the mooshroom, converting it into a normal cow and dropping vanilla loot.
    pub fn shear(&self, world: &Arc<World>, tool: &ItemStack) {
        world.play_sound_at(
            &sound_events::ENTITY_MOOSHROOM_SHEAR,
            SoundSource::Players,
            self.position(),
            DEFAULT_SOUND_VOLUME,
            DEFAULT_SOUND_PITCH,
            None,
        );

        let mut rng = rand::rng();
        for drop in shearing_loot_items_with_rng(
            self,
            &vanilla_loot_tables::SHEARING_MOOSHROOM,
            tool,
            &mut rng,
        ) {
            self.spawn_shearing_drop(&drop);
        }

        // TODO: Replace these local transfers with the shared conversion state transfer API.
        let cow = Arc::new(CowEntity::new(
            &vanilla_entities::COW,
            next_entity_id(),
            self.position(),
            Arc::downgrade(world),
        ));
        cow.set_rotation(self.rotation());
        cow.set_health(self.get_health());
        if let Some(custom_name) = self.custom_name() {
            cow.set_custom_name(Some(custom_name));
        }
        cow.set_custom_name_visible(self.is_custom_name_visible());
        cow.set_invulnerable(self.is_invulnerable());
        if self.is_no_ai() {
            cow.set_no_ai(true);
        }

        self.set_removed(RemovalReason::Discarded);
        let _ = world.try_add_entity(cow);
        world.send_particles(
            ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            self.position() + DVec3::Y * MOOSHROOM_CONVERSION_PARTICLE_HEIGHT_OFFSET,
            MOOSHROOM_CONVERSION_PARTICLE_COUNT,
            MOOSHROOM_CONVERSION_PARTICLE_SPREAD,
            MOOSHROOM_CONVERSION_PARTICLE_SPEED,
        );
    }

    fn spawn_shearing_drop(&self, drop: &ItemStack) {
        for _ in 0..drop.count() {
            let Some(item_entity) =
                self.spawn_at_location(drop.copy_with_count(1), SHEARING_DROP_HEIGHT_OFFSET)
            else {
                continue;
            };
            let jitter = DVec3::new(
                (rand::random::<f64>() - rand::random::<f64>()) * SHEARING_DROP_HORIZONTAL_JITTER,
                rand::random::<f64>() * SHEARING_DROP_VERTICAL_JITTER,
                (rand::random::<f64>() - rand::random::<f64>()) * SHEARING_DROP_HORIZONTAL_JITTER,
            );
            item_entity.set_velocity(item_entity.velocity() + jitter);
        }
    }

    fn try_interact_bowl(&self, player: &Player, hand: InteractionHand) -> bool {
        if AgeableMob::is_baby(self) {
            return false;
        }

        let is_bowl = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).is(&vanilla_items::BOWL)
        };
        if !is_bowl {
            return false;
        }

        let stew_effects = self.stew_effects.lock().take();
        let (stew_stack, sound) = if let Some(effects) = stew_effects {
            let mut stack = ItemStack::new(&vanilla_items::SUSPICIOUS_STEW);
            stack.set(SUSPICIOUS_STEW_EFFECTS, effects);
            (stack, &sound_events::ENTITY_MOOSHROOM_SUSPICIOUS_MILK)
        } else {
            (
                ItemStack::new(&vanilla_items::MUSHROOM_STEW),
                &sound_events::ENTITY_MOOSHROOM_MILK,
            )
        };

        self.play_sound(sound, DEFAULT_SOUND_VOLUME, DEFAULT_SOUND_PITCH);

        let overflow = {
            let mut inventory = player.inventory.lock();
            inventory.apply_filled_result(hand, stew_stack, player.has_infinite_materials(), false)
        };

        if !overflow.is_empty() {
            let _ = player.drop_item(overflow, false, false);
        }

        true
    }

    fn try_interact_flower(
        &self,
        player: &Player,
        hand: InteractionHand,
        item_stack: &ItemStack,
    ) -> InteractionResult {
        if self.variant() != MushroomCowVariant::Brown || AgeableMob::is_baby(self) {
            return InteractionResult::Pass;
        }

        let flower_effects = vanilla_suspicious_stew_effects::from_item(item_stack.item());

        let Some(effects) = flower_effects else {
            return InteractionResult::Pass;
        };

        if self.stew_effects.lock().is_some() {
            InteractionResult::SuccessServer
        } else {
            *self.stew_effects.lock() = Some(effects);
            self.play_sound(
                &sound_events::ENTITY_MOOSHROOM_EAT,
                MOOSHROOM_EAT_SOUND_VOLUME,
                DEFAULT_SOUND_PITCH,
            );
            Mob::use_player_item(self, player, hand);
            InteractionResult::SuccessServer
        }
    }

    fn try_interact_shears(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        if !self.ready_for_shearing() {
            return InteractionResult::Pass;
        }

        let Some(world) = self.level() else {
            return InteractionResult::Consume;
        };

        let tool = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).copy_with_count(1)
        };
        self.shear(&world, &tool);
        world.game_event_at(
            &vanilla_game_events::SHEAR,
            self.position(),
            &GameEventContext::new(Some(player as &dyn Entity), None),
        );
        player
            .inventory
            .lock()
            .hurt_item_in_hand(hand, 1, player.has_infinite_materials());

        InteractionResult::SuccessServer
    }

    fn try_milk(&self, player: &Player, hand: InteractionHand) -> bool {
        if AgeableMob::is_baby(self) {
            return false;
        }

        let is_bucket = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).is(&vanilla_items::BUCKET)
        };
        if !is_bucket {
            return false;
        }

        player.play_sound(
            &sound_events::ENTITY_COW_MILK,
            DEFAULT_SOUND_VOLUME,
            DEFAULT_SOUND_PITCH,
        );

        let overflow = {
            let mut inventory = player.inventory.lock();
            inventory.apply_filled_result(
                hand,
                ItemStack::new(&vanilla_items::MILK_BUCKET),
                player.has_infinite_materials(),
                true,
            )
        };

        if !overflow.is_empty() {
            let _ = player.drop_item(overflow, false, false);
        }

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
}

impl Entity for MushroomCowEntity {
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
            MOOSHROOM_BABY_DIMENSIONS.scale(scale)
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
        self.play_sound(
            &sound_events::ENTITY_COW_STEP,
            COW_STEP_SOUND_VOLUME,
            DEFAULT_SOUND_PITCH,
        );
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("Type", self.variant().serialized_name());
        if let Some(stew_effects) = self.stew_effects.lock().as_ref() {
            nbt.insert("stew_effects", stew_effects.clone().to_nbt_tag());
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

        if let Some(variant_name) = nbt.string("Type") {
            if let Some(variant) = MushroomCowVariant::from_name(variant_name.to_str().as_ref()) {
                self.set_variant(variant);
            }
        }

        if let Some(tag) = nbt.get("stew_effects") {
            *self.stew_effects.lock() = SuspiciousStewEffects::from_nbt_tag(tag);
        }
    }
}

impl LivingEntity for MushroomCowEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, health: f32) {
        let max_health = self.get_max_health();
        let clamped = health.clamp(MIN_HEALTH, max_health);
        self.entity_data
            .lock()
            .living_entity_mut()
            .health
            .set(clamped);
    }

    fn sound_volume(&self) -> f32 {
        MOOSHROOM_SOUND_VOLUME
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_COW_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_COW_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = Mob::mob_ai_step(self);

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        result
    }
}

impl AgeableMob for MushroomCowEntity {
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

impl Animal for MushroomCowEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        CowEntity::is_food(item_stack)
    }

    fn animal_walk_target_value(&self, pos: BlockPos) -> f32 {
        let Some(world) = self.level() else {
            return 0.0;
        };

        if world.get_block_state(pos.below()).get_block() == &vanilla_blocks::MYCELIUM {
            MOOSHROOM_WALK_TARGET_VALUE
        } else {
            world.pathfinding_cost_from_light_levels(pos)
        }
    }

    fn check_animal_spawn_rules(
        level: &dyn LevelReader,
        _spawn_reason: EntitySpawnReason,
        pos: BlockPos,
    ) -> bool
    where
        Self: Sized,
    {
        level
            .get_block_state(pos.below())
            .get_block()
            .has_tag(&BlockTag::MOOSHROOMS_SPAWNABLE_ON)
            && Self::is_bright_enough_to_spawn(level, pos)
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, offspring: &dyn Animal) {
        let mate_variant = partner
            .downcast_ref::<MushroomCowEntity>()
            .map_or(self.variant(), MushroomCowEntity::variant);

        let self_variant = self.variant();
        let baby_variant = if self_variant == mate_variant {
            if rand::random::<u32>() % MUTATE_CHANCE == 0 {
                self_variant.opposite()
            } else {
                self_variant
            }
        } else if rand::random::<bool>() {
            self_variant
        } else {
            mate_variant
        };

        if let Some(baby) = offspring.downcast_ref::<MushroomCowEntity>() {
            baby.set_variant(baby_variant);
        }
    }
}

impl Mob for MushroomCowEntity {
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
        Some(&sound_events::ENTITY_COW_AMBIENT)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.set_variant(MushroomCowVariant::Red);
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        if self.try_interact_bowl(player, hand) {
            return InteractionResult::Success;
        }

        let item_stack = {
            let inventory = player.inventory.lock();
            let stack = inventory.get_item_in_hand(hand);
            stack.copy_with_count(stack.count())
        };

        if item_stack.is(&vanilla_items::SHEARS) {
            let result = self.try_interact_shears(player, hand);
            if result != InteractionResult::Pass {
                return result;
            }
        }

        let flower_result = self.try_interact_flower(player, hand, &item_stack);
        if flower_result != InteractionResult::Pass {
            return flower_result;
        }

        if self.try_milk(player, hand) {
            return InteractionResult::Success;
        }

        Animal::mob_interact_animal(self, player, hand)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for MushroomCowEntity {}

#[cfg(test)]
mod tests;
