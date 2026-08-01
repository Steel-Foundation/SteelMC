//! Pig entity implementation.
//!
//! This is the first concrete pathfinder mob foundation.

use std::str::FromStr;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::pig_sound_variant::{PigAge, PigSoundVariantRef};
use steel_registry::pig_variant::PigVariantRef;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::PigEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_items,
    vanilla_pig_sound_variants, vanilla_pig_variants,
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
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, ItemBasedSteering, ItemSteerable, LivingEntity,
    LivingEntityBase, Mob, MobBase, PathfinderMob, SharedEntity, SpawnGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::player::Player;
use crate::world::World;

const PIG_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.5, 0.0)];
const PIG_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    0.45,
    0.45,
    0.40625,
    EntityAttachments::new(&PIG_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);

/// Vanilla pig entity.
#[entity_behavior(class = "Pig")]
pub struct PigEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    steering: SyncMutex<ItemBasedSteering>,
    entity_data: SyncMutex<PigEntityData>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `PigEntity`.
unsafe impl DowncastType for PigEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/pig");
}

impl PigEntity {
    /// Creates a new pig entity.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates a pig entity from saved base data.
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
        let steering = SyncMutex::new(ItemBasedSteering::new());
        let mut entity_data = PigEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, FloatGoal::new(&mob_base));
            goal_selector.add_goal(1, PanicGoal::new(1.25));
            goal_selector.add_goal(3, BreedGoal::new(1.0));
            goal_selector.add_goal(
                4,
                TemptGoal::new(
                    1.2,
                    |item_stack| item_stack.is(&vanilla_items::CARROT_ON_A_STICK),
                    false,
                ),
            );
            goal_selector.add_goal(
                4,
                TemptGoal::new(
                    1.2,
                    |item_stack| {
                        REGISTRY
                            .items
                            .is_in_tag(item_stack.item(), &ItemTag::PIG_FOOD)
                    },
                    false,
                ),
            );
            goal_selector.add_goal(5, FollowParentGoal::new(1.1));
            goal_selector.add_goal(6, WaterAvoidingRandomStrollGoal::new(1.0));
            goal_selector.add_goal(7, LookAtPlayerGoal::new(6.0));
            goal_selector.add_goal(8, RandomLookAroundGoal::new());
        }

        Self {
            base,
            entity_type,
            living_base,
            mob_base,
            ageable_base,
            animal_base,
            steering,
            entity_data: SyncMutex::new(entity_data),
        }
    }

    /// Returns the current pig variant registry ID stored in synced data.
    #[must_use]
    pub fn variant_id(&self) -> i32 {
        *self.entity_data.lock().variant.get()
    }

    /// Sets the current pig variant by registry entry.
    pub fn set_variant(&self, variant: PigVariantRef) {
        let Some(id) = REGISTRY.pig_variants.id_from_key(&variant.key) else {
            log::error!("pig variant {} is not registered", variant.key);
            return;
        };
        self.set_variant_id_from_usize(id);
    }

    /// Returns the current pig variant, falling back to vanilla's default holder.
    #[must_use]
    pub fn variant(&self) -> PigVariantRef {
        let id = self.variant_id();
        if let Ok(id) = usize::try_from(id)
            && let Some(variant) = REGISTRY.pig_variants.by_id(id)
        {
            return variant;
        }

        &vanilla_pig_variants::TEMPERATE
    }

    /// Returns the current pig sound variant registry ID stored in synced data.
    #[must_use]
    pub fn sound_variant_id(&self) -> i32 {
        *self.entity_data.lock().sound_variant.get()
    }

    /// Sets the current pig sound variant by registry entry.
    pub fn set_sound_variant(&self, sound_variant: PigSoundVariantRef) {
        let Some(id) = REGISTRY.pig_sound_variants.id_from_key(&sound_variant.key) else {
            log::error!("pig sound variant {} is not registered", sound_variant.key);
            return;
        };
        self.set_sound_variant_id_from_usize(id);
    }

    /// Returns the current pig sound variant, falling back to vanilla classic.
    #[must_use]
    pub fn sound_variant(&self) -> PigSoundVariantRef {
        let id = self.sound_variant_id();
        if let Ok(id) = usize::try_from(id)
            && let Some(sound_variant) = REGISTRY.pig_sound_variants.by_id(id)
        {
            return sound_variant;
        }

        &vanilla_pig_sound_variants::CLASSIC
    }

    fn set_variant_id_from_usize(&self, id: usize) {
        let Ok(id) = i32::try_from(id) else {
            log::error!("pig variant id {id} does not fit synced-data i32");
            return;
        };
        self.entity_data.lock().variant.set(id);
    }

    fn set_sound_variant_id_from_usize(&self, id: usize) {
        let Ok(id) = i32::try_from(id) else {
            log::error!("pig sound variant id {id} does not fit synced-data i32");
            return;
        };
        self.entity_data.lock().sound_variant.set(id);
    }

    fn set_variant_by_key(&self, key: &Identifier) -> bool {
        let Some(id) = REGISTRY.pig_variants.id_from_key(key) else {
            return false;
        };
        self.set_variant_id_from_usize(id);
        true
    }

    fn set_sound_variant_by_key(&self, key: &Identifier) {
        if let Some(id) = REGISTRY.pig_sound_variants.id_from_key(key) {
            self.set_sound_variant_id_from_usize(id);
        }
    }

    fn current_sound_set(&self) -> &'static PigAge {
        let sound_variant = self.sound_variant();
        if AgeableMob::is_baby(self) {
            &sound_variant.baby_sounds
        } else {
            &sound_variant.adult_sounds
        }
    }

    /// Returns whether this pig has a saddle equipped.
    #[must_use]
    pub fn is_saddled(&self) -> bool {
        LivingEntity::has_item_in_slot(self, EquipmentSlot::Saddle)
    }

    /// Returns whether this pig can currently use the saddle equipment slot.
    #[must_use]
    pub fn can_use_saddle_slot(&self) -> bool {
        Entity::is_alive(self) && !AgeableMob::is_baby(self)
    }

    /// Returns whether item-based steering is currently boosting.
    #[must_use]
    pub fn is_boosting(&self) -> bool {
        self.steering.lock().is_boosting()
    }

    /// Returns the current elapsed boost time.
    #[must_use]
    pub fn elapsed_boost_time(&self) -> i32 {
        self.steering.lock().boost_time()
    }

    /// Returns vanilla pig ridden speed.
    #[must_use]
    pub fn ridden_speed(&self) -> f32 {
        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED) as f32;
        movement_speed * 0.225 * ItemSteerable::boost_factor(self)
    }

    fn set_ridden_rotation(&self, controller_yaw: f32, controller_pitch: f32) {
        self.set_rotation((controller_yaw, controller_pitch * 0.5));
        self.base.set_old_yaw_to_current();
        let yaw = self.rotation().0;
        self.set_y_body_rot(yaw);
        self.set_y_head_rot(yaw);
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

    /// Returns whether the stack is vanilla pig food.
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::PIG_FOOD)
    }
}

impl Entity for PigEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if AgeableMob::is_baby(self) {
            PIG_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        if self.is_saddled()
            && let Some(passenger) = self.first_passenger()
            && passenger.as_player().is_some_and(|player| {
                let mut is_holding_carrot_on_a_stick =
                    |item_stack: &ItemStack| item_stack.is(&vanilla_items::CARROT_ON_A_STICK);
                player.is_holding(&mut is_holding_carrot_on_a_stick)
            })
        {
            return Some(passenger);
        }

        self.controlling_passenger_mob()
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

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        nbt.insert("variant", self.variant().key.to_string());
        nbt.insert("sound_variant", self.sound_variant().key.to_string());
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);

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

impl LivingEntity for PigEntity {
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

    fn can_use_slot(&self, slot: EquipmentSlot) -> bool {
        slot != EquipmentSlot::Saddle || self.can_use_saddle_slot()
    }

    fn can_dispenser_equip_into_slot(&self, slot: EquipmentSlot) -> bool {
        slot == EquipmentSlot::Saddle || Mob::can_pick_up_loot(self)
    }

    fn equip_sound(&self, slot: EquipmentSlot, _stack: &ItemStack) -> Option<SoundEventRef> {
        (slot == EquipmentSlot::Saddle).then_some(&sound_events::ENTITY_PIG_SADDLE)
    }

    fn tick_ridden(&self, controller: &Player, _ridden_input: DVec3) {
        let (yaw, pitch) = controller.rotation();
        self.set_ridden_rotation(yaw, pitch);
        ItemSteerable::tick_boost(self);
    }

    fn ridden_input(&self, _controller: &Player, _self_input: DVec3) -> DVec3 {
        DVec3::new(0.0, 0.0, 1.0)
    }

    fn ridden_speed(&self, _controller: &Player) -> f32 {
        PigEntity::ridden_speed(self)
    }
}

impl AgeableMob for PigEntity {
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
}

impl Animal for PigEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        PigEntity::is_food(item_stack)
    }

    fn play_eating_sound(&self) {
        self.play_sound(self.current_sound_set().eat_sound, 1.0, 1.0);
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
            log::error!("pig offspring could not inherit breeding variant {variant_key}");
        }
    }
}

impl ItemSteerable for PigEntity {
    fn item_based_steering(&self) -> &SyncMutex<ItemBasedSteering> {
        &self.steering
    }

    fn boost_time_total(&self) -> i32 {
        *self.entity_data.lock().boost_time.get()
    }

    fn set_boost_time_total(&self, boost_time_total: i32) {
        self.entity_data.lock().boost_time.set(boost_time_total);
    }
}

impl Mob for PigEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(self.current_sound_set().ambient_sound)
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        spawn_reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let biome = world.biome_at(self.block_position());
        let (variant, sound_variant) = {
            let mut random = LegacyRandom::from_seed(rand::random());
            let variant = biome.and_then(|biome| {
                REGISTRY
                    .pig_variants
                    .select_spawn_variant(biome, &mut random)
            });
            let sound_variant = REGISTRY.pig_sound_variants.pick_random(&mut random);
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
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };
        let has_food = PigEntity::is_food(&item_stack);

        if !has_food && self.is_saddled() && !self.is_vehicle() && !player.is_secondary_use_active()
        {
            if let Some(world) = self.level()
                && let Some(vehicle) = world.get_entity_by_id(self.id())
            {
                player.start_riding(&vehicle);
            }
            return InteractionResult::Success;
        }

        let interaction_result = Animal::mob_interact_animal(self, player, hand);
        if interaction_result.consumes_action() {
            return interaction_result;
        }

        if LivingEntity::is_equippable_in_slot(self, &item_stack, EquipmentSlot::Saddle) {
            return LivingEntity::interact_living_entity_with_equippable(self, player, hand);
        }

        InteractionResult::Pass
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, flags: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(flags);
    }
}

impl PathfinderMob for PigEntity {}

#[cfg(test)]
mod tests;
