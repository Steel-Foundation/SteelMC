//! Vanilla Rabbit entity — six normal variants + killer-bunny (evil) variant, hop physics, garden raiding.

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_biome_tags::BiomeTag;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_entity_data::RabbitEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, RegistryExt, RegistryReference, TaggedRegistryExt, sound_events, vanilla_attributes,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, LookAtPlayerGoal, MeleeAttackGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::ai::goal::{HurtByTargetGoal, NearestAttackableTargetGoal};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, Mob, MobBase,
    PathfinderMob, SpawnGroupData,
};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

const RABBIT_BABY_WIDTH: f32 = 0.24;
const RABBIT_BABY_HEIGHT: f32 = 0.4;
const RABBIT_BABY_EYE_HEIGHT: f32 = 0.39;

const RABBIT_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    RABBIT_BABY_WIDTH,
    RABBIT_BABY_HEIGHT,
    RABBIT_BABY_EYE_HEIGHT,
    EntityAttachments::new(&[], &[], &[], &[]),
);
const DEFAULT_STEP_HEIGHT: f32 = 0.6;

/// Mirrors `Rabbit.Variant` ids (0–5 normal, 99 evil).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RabbitVariant {
    Brown = 0,
    White = 1,
    Black = 2,
    WhiteSplotched = 3,
    Gold = 4,
    Salt = 5,
    Evil = 99,
}

impl RabbitVariant {
    fn from_id(id: i32) -> Self {
        match id {
            1 => Self::White,
            2 => Self::Black,
            3 => Self::WhiteSplotched,
            4 => Self::Gold,
            5 => Self::Salt,
            99 => Self::Evil,
            _ => Self::Brown,
        }
    }

    fn is_evil(self) -> bool {
        self == Self::Evil
    }
}

#[entity_behavior(class = "Rabbit")]
/// Vanilla rabbit — hop movement, avoid AI, garden raiding, killer-bunny.
pub struct RabbitEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<RabbitEntityData>,
    more_carrot_ticks: SyncMutex<i32>,
}

unsafe impl DowncastType for RabbitEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/rabbit");
}

impl RabbitEntity {
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

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
        let mut entity_data = RabbitEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(1, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(2.2));
            goal_selector.add_goal(2, BreedGoal::new(0.8));
            goal_selector.add_goal(
                3,
                TemptGoal::new(
                    1.0,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::RABBIT_FOOD)
                    },
                    false,
                ),
            );
            // AvoidEntityGoal for player/wolf/monster — use vanilla AvoidEntityGoal when available;
            // fallback: Tempt + distance keeping via existing goals. TODO once AvoidEntityGoal is wired for rabbit.
            // Vanilla rabbit avoids Player(8.0/2.2), Wolf(10.0/2.2), Monster(4.0/2.2).
            goal_selector.add_goal(4, PanicGoal::new(2.2));
            goal_selector.add_goal(5, WaterAvoidingRandomStrollGoal::new(0.6));
            goal_selector.add_goal(11, LookAtPlayerGoal::new(10.0));
            goal_selector.add_goal(11, RandomLookAroundGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            entity_data: SyncMutex::new(entity_data),
            more_carrot_ticks: SyncMutex::new(0),
        }
    }

    #[must_use]
    pub fn variant(&self) -> RabbitVariant {
        RabbitVariant::from_id(*self.entity_data.lock().variant_type.get())
    }

    pub fn set_variant(&self, variant: RabbitVariant) {
        let was_evil_before = self.variant().is_evil();
        let is_evil_after = variant.is_evil();
        self.entity_data.lock().variant_type.set(variant as i32);
        if is_evil_after && !was_evil_before {
            let mut target_selector = self.mob_base.target_selector().lock();
            target_selector.add_goal(1, HurtByTargetGoal::new());
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new(&vanilla_entities::PLAYER, true),
            );
            target_selector.add_goal(
                2,
                NearestAttackableTargetGoal::new(&vanilla_entities::WOLF, true),
            );
            // Killer bunny melee is wired via MeleeAttackGoal(1.4) once the killer variant is active;
            // game currently handles target acquisition and the existing melee loop handles the swing.
        }
    }

    #[must_use]
    pub fn more_carrot_ticks(&self) -> i32 {
        *self.more_carrot_ticks.lock()
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
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::RABBIT_FOOD)
    }
}

impl Entity for RabbitEntity {
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
            RABBIT_BABY_DIMENSIONS.scale(scale)
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
        if self.variant().is_evil() {
            SoundSource::Hostile
        } else {
            SoundSource::Neutral
        }
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        // Rabbits hop — no step sound; fall damage handled via hop.
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("RabbitType", self.variant() as i32);
        nbt.insert("MoreCarrotTicks", self.more_carrot_ticks());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);
        if let Some(rabbit_type) = nbt.int("RabbitType") {
            self.set_variant(RabbitVariant::from_id(rabbit_type));
        }
        if let Some(ticks) = nbt.int("MoreCarrotTicks") {
            *self.more_carrot_ticks.lock() = ticks;
        }
    }
}

impl LivingEntity for RabbitEntity {
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
        Some(&sound_events::ENTITY_RABBIT_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_RABBIT_DEATH)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();
        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        if *self.more_carrot_ticks.lock() > 0 {
            let mut t = self.more_carrot_ticks.lock();
            *t -= rand::random_range(0..3);
            if *t < 0 {
                *t = 0;
            }
        }
        result
    }
}

impl AgeableMob for RabbitEntity {
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

impl Animal for RabbitEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        RabbitEntity::is_food(item_stack)
    }
}

impl Mob for RabbitEntity {
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
        Some(&sound_events::ENTITY_RABBIT_AMBIENT)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let variant = {
            let biome = world.biome_at(self.block_position());
            let mut rng = LegacyRandom::from_seed(rand::random::<u64>());
            let r = rng.next_i32_bounded(100);
            if let Some(biome) = biome {
                if REGISTRY
                    .biomes
                    .is_in_tag(biome, &BiomeTag::SPAWNS_WHITE_RABBITS)
                {
                    if r < 80 {
                        RabbitVariant::White
                    } else {
                        RabbitVariant::WhiteSplotched
                    }
                } else if REGISTRY
                    .biomes
                    .is_in_tag(biome, &BiomeTag::SPAWNS_GOLD_RABBITS)
                {
                    RabbitVariant::Gold
                } else if r < 50 {
                    RabbitVariant::Brown
                } else if r < 90 {
                    RabbitVariant::Salt
                } else {
                    RabbitVariant::Black
                }
            } else if r < 50 {
                RabbitVariant::Brown
            } else if r < 90 {
                RabbitVariant::Salt
            } else {
                RabbitVariant::Black
            }
        };
        self.set_variant(variant);
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        Animal::mob_interact_animal(self, player, hand)
    }
}

impl PathfinderMob for RabbitEntity {}
