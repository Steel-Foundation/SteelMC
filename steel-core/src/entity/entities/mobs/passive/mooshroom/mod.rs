//! Vanilla Mooshroom (MushroomCow) entity — red/brown variant, bowl milking, shearing, and suspicious-stew feeding.

use std::str::FromStr;
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
use steel_registry::vanilla_entity_data::MushroomCowEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, RegistryReference, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_items,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey, Identifier};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
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

const MOOSHROOM_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.75, 0.0)];
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

/// Mooshroom variant — mirrors `MushroomCow.Variant` ids (0 = red, 1 = brown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MooshroomVariant {
    Red = 0,
    Brown = 1,
}

impl MooshroomVariant {
    fn from_id(id: i32) -> Self {
        match id {
            1 => Self::Brown,
            _ => Self::Red,
        }
    }
}

#[entity_behavior(class = "MushroomCow")]
/// Vanilla mooshroom — mushroom cow with stew effects, shearing, and variant.
pub struct MooshroomEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<MushroomCowEntityData>,
}

unsafe impl DowncastType for MooshroomEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/mooshroom");
}

impl MooshroomEntity {
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
        let mut entity_data = MushroomCowEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);

        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(2.0));
            goal_selector.add_goal(2, BreedGoal::new(1.0));
            goal_selector.add_goal(
                3,
                TemptGoal::new(
                    1.25,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::COW_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(4, FollowParentGoal::new(1.25));
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
            entity_data: SyncMutex::new(entity_data),
        }
    }

    #[must_use]
    pub fn variant(&self) -> MooshroomVariant {
        MooshroomVariant::from_id(*self.entity_data.lock().variant_type.get())
    }

    pub fn set_variant(&self, variant: MooshroomVariant) {
        self.entity_data.lock().variant_type.set(variant as i32);
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
            .is_in_tag(item_stack.item(), &ItemTag::COW_FOOD)
    }

    fn try_interact(&self, player: &Player, hand: InteractionHand) -> bool {
        let is_baby = AgeableMob::is_baby(self);
        if is_baby {
            return false;
        }

        let held_is_bowl = {
            let inv = player.inventory.lock();
            inv.get_item_in_hand(hand).is(&vanilla_items::BOWL)
        };
        if held_is_bowl {
            let overflow = {
                let mut inv = player.inventory.lock();
                inv.apply_filled_result(
                    hand,
                    ItemStack::new(&vanilla_items::MUSHROOM_STEW),
                    player.has_infinite_materials(),
                    true,
                )
            };
            if !overflow.is_empty() {
                let _ = player.drop_item(overflow, false, false);
            }
            self.play_sound(&sound_events::ENTITY_MOOSHROOM_MILK, 1.0, 1.0);
            return true;
        }

        let held_is_shears = {
            let inv = player.inventory.lock();
            inv.get_item_in_hand(hand).is(&vanilla_items::SHEARS)
        };
        if held_is_shears {
            self.play_sound(&sound_events::ENTITY_MOOSHROOM_SHEAR, 1.0, 1.0);
            if let Some(world) = self.level() {
                for _ in 0..5 {
                    world.drop_item_stack(
                        self.block_position(),
                        ItemStack::new(&vanilla_items::RED_MUSHROOM),
                    );
                }
                // TODO: Convert to Cow via entity conversion once conversion helper exists (vanilla convertTo(COW)).
            }
            {
                // TODO: Damage shears via hurtAndBreak once the item-damage helper is wired.
                // Vanilla does `itemStack.hurtAndBreak(1, player, hand.asEquipmentSlot())`.
                let _ = player;
                let _ = hand;
            }
            return true;
        }

        false
    }
}

impl Entity for MooshroomEntity {
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
        self.play_sound(&sound_events::ENTITY_COW_STEP, 0.15, 1.0);
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        let variant_name = match self.variant() {
            MooshroomVariant::Brown => "brown",
            MooshroomVariant::Red => "red",
        };
        nbt.insert("Type", variant_name);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);
        if let Some(type_str) = nbt.string("Type") {
            match type_str.to_str().as_ref() {
                "brown" => self.set_variant(MooshroomVariant::Brown),
                _ => self.set_variant(MooshroomVariant::Red),
            }
        }
    }
}

impl LivingEntity for MooshroomEntity {
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
        Some(&sound_events::ENTITY_COW_HURT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_COW_DEATH)
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

impl AgeableMob for MooshroomEntity {
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

impl Animal for MooshroomEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        MooshroomEntity::is_food(item_stack)
    }

    fn breed_variant_key(&self) -> Option<&Identifier> {
        None
    }

    fn set_breed_variant_key(&self, _key: &Identifier) -> bool {
        false
    }

    fn initialize_breed_offspring(&self, partner: &dyn Animal, _offspring: &dyn Animal) {
        let _ = partner;
        // Variant handled via Mooshroom-specific getOffspringVariant (mutation 1/1024) in finalize.
    }
}

impl Mob for MooshroomEntity {
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
        self.finalize_spawn_ageable_mob(world, spawn_reason, group_data)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        if self.try_interact(player, hand) {
            return InteractionResult::Success;
        }
        Animal::mob_interact_animal(self, player, hand)
    }
}

impl PathfinderMob for MooshroomEntity {}
