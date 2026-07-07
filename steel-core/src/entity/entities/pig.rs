//! Pig entity implementation.
//!
//! This is the first concrete pathfinder mob foundation.

use std::str::FromStr;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::{entity_behavior, entity_impl};
use steel_protocol::packets::game::{AttributeSnapshot, EquipmentSlotItem, SoundSource};
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
    REGISTRY, RegistryEntry, RegistryExt, TaggedRegistryExt, sound_events, vanilla_attributes,
    vanilla_items, vanilla_particle_types, vanilla_pig_sound_variants, vanilla_pig_variants,
};
use steel_utils::locks::SyncMutex;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, Identifier};

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FloatGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal, RandomLookAroundGoal,
    TemptGoal, WaterAvoidingRandomStrollGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, ItemBasedSteering, ItemSteerable, LivingEntity,
    LivingEntityBase, Mob, MobBase, MobEffectSyncChange, PathfinderMob, SharedEntity,
    SpawnGroupData,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
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
                    |item_stack| item_stack.is(&vanilla_items::ITEMS.carrot_on_a_stick),
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

    /// Returns the vanilla age counter. Negative values are babies.
    #[must_use]
    pub fn get_age(&self) -> i32 {
        AgeableMob::get_age(self)
    }

    /// Sets the vanilla age counter and updates the synchronized baby flag.
    pub fn set_age(&self, age: i32) {
        AgeableMob::set_age(self, age);
    }

    /// Returns whether this pig is a baby.
    #[must_use]
    pub fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
    }

    /// Sets the vanilla baby state using the `AgeableMob` start age.
    pub fn set_baby(&self, baby: bool) {
        AgeableMob::set_baby(self, baby);
    }

    /// Returns vanilla `AgeableMob.forcedAge`.
    #[must_use]
    pub fn forced_age(&self) -> i32 {
        AgeableMob::forced_age(self)
    }

    /// Sets vanilla `AgeableMob.forcedAge`.
    pub fn set_forced_age(&self, forced_age: i32) {
        AgeableMob::set_forced_age(self, forced_age);
    }

    /// Returns the synchronized vanilla age-lock flag.
    #[must_use]
    pub fn is_age_locked(&self) -> bool {
        AgeableMob::is_age_locked(self)
    }

    /// Sets the synchronized vanilla age-lock flag.
    pub fn set_age_locked(&self, age_locked: bool) {
        AgeableMob::set_age_locked(self, age_locked);
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
        if self.is_baby() {
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
        Entity::is_alive(self) && !self.is_baby()
    }

    /// Returns the synced vanilla `DATA_BOOST_TIME` value.
    #[must_use]
    pub fn boost_time_total(&self) -> i32 {
        ItemSteerable::boost_time_total(self)
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

    /// Advances the active item-based steering boost.
    pub fn tick_boost(&self) {
        ItemSteerable::tick_boost(self);
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

        let Some(particle_type_id) = vanilla_particle_types::ENTITY_EFFECT.try_id() else {
            log::error!("vanilla entity_effect particle type is not registered");
            return;
        };
        let Ok(particle_type_id) = i32::try_from(particle_type_id) else {
            log::error!("vanilla entity_effect particle type id does not fit protocol i32");
            return;
        };
        let display = self.living_base.mob_effect_display_state(particle_type_id);

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

#[entity_impl(class(animal), interfaces(item_steerable))]
impl Entity for PigEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let scale = LivingEntity::get_scale(self);
        if self.is_baby() {
            PIG_BABY_DIMENSIONS.scale(scale)
        } else if self.entity_type.fixed {
            self.entity_type.dimensions
        } else {
            self.entity_type.dimensions.scale(scale)
        }
    }

    fn tick(&self) {
        self.default_tick();
        self.living_base.decrement_invulnerable_time();
        self.tick_mob_effects();

        if self.is_dead_or_dying() {
            LivingEntity::tick_death(self);
            self.tick_living_state();
            return;
        }

        if !self.is_removed() {
            self.ai_step();
        }

        self.tick_living_state();
    }

    fn check_despawn(&self) {
        Mob::check_mob_despawn(self);
    }

    fn is_alive(&self) -> bool {
        !self.is_removed() && self.get_health() > 0.0
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed()
    }

    fn is_pushable(&self) -> bool {
        Entity::is_alive(self) && !self.is_spectator() && !self.on_climbable()
    }

    fn controlling_passenger(&self) -> Option<SharedEntity> {
        if self.is_saddled()
            && let Some(passenger) = self.first_passenger()
            && passenger.as_player().is_some_and(|player| {
                let mut is_holding_carrot_on_a_stick =
                    |item_stack: &ItemStack| item_stack.is(&vanilla_items::ITEMS.carrot_on_a_stick);
                player.is_holding(&mut is_holding_carrot_on_a_stick)
            })
        {
            return Some(passenger);
        }

        self.controlling_passenger_mob()
    }

    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement() && !self.is_no_ai()
    }

    fn get_default_gravity(&self) -> f64 {
        LivingEntity::get_attribute_gravity(self)
    }

    fn can_freeze(&self) -> bool {
        self.default_living_can_freeze()
    }

    fn can_walk_on_powder_snow(&self) -> bool {
        self.default_living_can_walk_on_powder_snow()
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn update_data_before_sync(&self) {
        self.update_dirty_mob_effect_entity_data();
    }

    fn pack_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().syncable_snapshots()
    }

    fn drain_dirty_syncable_attributes(&self) -> Vec<AttributeSnapshot> {
        self.attributes().lock().drain_dirty_sync()
    }

    fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.living_base.drain_dirty_mob_effects()
    }

    fn pack_all_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.pack_living_equipment()
    }

    fn drain_dirty_equipment(&self) -> Vec<EquipmentSlotItem> {
        self.drain_dirty_living_equipment()
    }

    fn max_up_step(&self) -> f32 {
        self.attributes()
            .lock()
            .get_value(vanilla_attributes::STEP_HEIGHT)
            .unwrap_or(0.6) as f32
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn play_step_sound(&self, _pos: BlockPos, _block_state: BlockStateId) {
        self.play_sound(self.current_sound_set().step_sound, 0.15, 1.0);
    }

    fn hurt(&self, source: &DamageSource, amount: f32) -> bool {
        LivingEntity::hurt_server(self, source, amount)
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        Mob::interact_mob(self, player, hand, location)
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

    fn is_baby(&self) -> bool {
        AgeableMob::is_baby(self)
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

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn tick_ridden(&self, controller: &Player, _ridden_input: DVec3) {
        let (yaw, pitch) = controller.rotation();
        self.set_ridden_rotation(yaw, pitch);
        self.tick_boost();
    }

    fn ridden_input(&self, _controller: &Player, _self_input: DVec3) -> DVec3 {
        DVec3::new(0.0, 0.0, 1.0)
    }

    fn ridden_speed(&self, _controller: &Player) -> f32 {
        PigEntity::ridden_speed(self)
    }

    fn before_actually_hurt(&self, _source: &DamageSource, _amount: f32) {
        Animal::reset_love(self);
    }

    fn ai_step(&self) -> Option<MoveResult> {
        let result = self.default_ai_step();

        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        result
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

    fn age_boundary_changed(&self, _baby: bool) {
        self.refresh_dimensions();
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

    fn remove_when_far_away(&self, dist_sqr: f64) -> bool {
        Animal::remove_when_far_away_animal(self, dist_sqr)
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
mod tests {
    use std::io::Cursor;
    use std::string::ToString;

    use simdnbt::borrow::read_compound as read_borrowed_compound;
    use simdnbt::owned::NbtTag;
    use steel_registry::entity_type::EntityAttachment;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{
        vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_items::ITEMS,
    };
    use steel_utils::UuidExt;
    use uuid::Uuid;

    use crate::entity::ai::navigation::NavigationTickContext;
    use crate::entity::ai::node::Node;
    use crate::entity::ai::path::{Path, PathType};
    use crate::entity::damage::DamageSource;
    use crate::entity::entities::LeashFenceKnotEntity;
    use crate::entity::mob::LeashAttachment;
    use crate::entity::{Animal, DEATH_DURATION, ItemSteerable, RemovalReason, SharedEntity};
    use crate::inventory::equipment::EquipmentSlot;
    use crate::world::LevelReader;

    use super::*;

    struct EmptyNavigationLevel {
        air_state: BlockStateId,
    }

    impl EmptyNavigationLevel {
        fn new() -> Self {
            Self {
                air_state: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
            }
        }
    }

    impl LevelReader for EmptyNavigationLevel {
        fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
            self.air_state
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn pig_initializes_vanilla_living_attributes_and_health() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert_eq!(pig.get_health().to_bits(), 10.0_f32.to_bits());
        let attributes = pig.attributes().lock();
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::MAX_HEALTH)
                .to_bits(),
            10.0_f64.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::MOVEMENT_SPEED)
                .to_bits(),
            0.25_f64.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::FOLLOW_RANGE)
                .to_bits(),
            16.0_f64.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::TEMPT_RANGE)
                .to_bits(),
            10.0_f64.to_bits()
        );
    }

    #[test]
    fn pig_exposes_living_entity_behavior_without_downcasting() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let entity = &pig as &dyn Entity;

        assert!(entity.is_living_entity());
        let Some(living) = entity.as_living_entity() else {
            panic!("pig should expose living behavior");
        };
        assert_eq!(living.get_health().to_bits(), 10.0_f32.to_bits());
    }

    #[test]
    fn pig_exposes_pathfinder_mob_behavior_without_downcasting() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let entity = &pig as &dyn Entity;

        assert!(entity.is_pathfinder_mob());
        let Some(pathfinder) = entity.as_pathfinder_mob() else {
            panic!("pig should expose pathfinder behavior");
        };
        assert!(!pathfinder.is_path_finding());
    }

    #[test]
    fn pig_exposes_mob_behavior_without_downcasting() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let entity = &pig as &dyn Entity;

        assert!(entity.is_mob());
        let Some(mob) = entity.as_mob() else {
            panic!("pig should expose mob behavior");
        };
        assert_eq!(
            mob.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
            0.085_f32.to_bits()
        );
    }

    #[test]
    fn pig_exposes_animal_behavior_without_downcasting() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let entity = &pig as &dyn Entity;

        assert!(entity.is_animal());
        let Some(animal) = entity.as_animal() else {
            panic!("pig should expose animal behavior");
        };
        animal.set_in_love_time(5);
        assert_eq!(animal.in_love_time(), 5);
        assert!(animal.is_in_love());
    }

    #[test]
    fn pig_exposes_item_steerable_behavior_without_downcasting() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let entity = &pig as &dyn Entity;

        assert!(entity.is_item_steerable());
        let Some(steerable) = entity.as_item_steerable() else {
            panic!("pig should expose item-steerable behavior");
        };
        assert_eq!(steerable.boost_time_total(), 0);
    }

    #[test]
    fn pig_item_steerable_boost_updates_synced_total_once() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(ItemSteerable::boost(&pig));
        let boost_time_total = pig.boost_time_total();

        assert!((140..=980).contains(&boost_time_total));
        assert!(pig.is_boosting());
        assert_eq!(pig.elapsed_boost_time(), 0);
        assert!(!ItemSteerable::boost(&pig));
        assert_eq!(pig.boost_time_total(), boost_time_total);
    }

    #[test]
    fn pig_ridden_speed_uses_item_steering_boost_factor() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let base_ridden_speed = 0.25_f32 * 0.225;

        assert_eq!(pig.ridden_speed().to_bits(), base_ridden_speed.to_bits());

        assert!(ItemSteerable::boost(&pig));
        pig.tick_boost();

        assert!(pig.ridden_speed() > base_ridden_speed);
    }

    #[test]
    fn pig_ridden_rotation_matches_controller_head_and_body_yaw() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.base().set_old_rotation((7.0, -12.0));

        pig.set_ridden_rotation(450.0, 120.0);

        assert_eq!(pig.rotation(), (90.0, 60.0));
        assert_eq!(pig.base().old_rotation(), (90.0, -12.0));
        assert_eq!(pig.y_body_rot().to_bits(), 90.0_f32.to_bits());
        assert_eq!(pig.y_head_rot().to_bits(), 90.0_f32.to_bits());
    }

    #[test]
    fn pig_can_mate_with_same_type_when_both_in_love() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let partner = PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(1.0, 0.0, 0.0),
            Weak::new(),
        );

        assert!(!pig.can_mate(&partner));

        pig.set_in_love_time(20);
        partner.set_in_love_time(20);

        assert!(pig.can_mate(&partner));
        assert!(!pig.can_mate(&pig));
    }

    #[test]
    fn pig_uses_default_animal_love_mode() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(pig.can_fall_in_love());

        pig.set_in_love(None);

        assert_eq!(pig.in_love_time(), 600);
        assert!(!pig.can_fall_in_love());
        assert!(pig.love_cause_uuid().is_none());
    }

    #[test]
    fn pig_saddle_slot_requires_alive_adult() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let saddle = ItemStack::new(&ITEMS.saddle);

        assert!(LivingEntity::is_equippable_in_slot(
            &pig,
            &saddle,
            EquipmentSlot::Saddle
        ));

        pig.set_baby(true);
        assert!(!LivingEntity::is_equippable_in_slot(
            &pig,
            &saddle,
            EquipmentSlot::Saddle
        ));

        pig.set_baby(false);
        pig.set_health(0.0);
        assert!(!LivingEntity::is_equippable_in_slot(
            &pig,
            &saddle,
            EquipmentSlot::Saddle
        ));
    }

    #[test]
    fn pig_dispenser_can_equip_saddle_only_when_alive_adult_and_empty() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let saddle = ItemStack::new(&ITEMS.saddle);

        assert!(LivingEntity::can_equip_with_dispenser(&pig, &saddle));

        pig.living_base
            .equipment()
            .lock()
            .set(EquipmentSlot::Saddle, ItemStack::new(&ITEMS.saddle));
        assert!(!LivingEntity::can_equip_with_dispenser(&pig, &saddle));

        let baby = PigEntity::new(&vanilla_entities::PIG, 2, DVec3::ZERO, Weak::new());
        baby.set_baby(true);
        assert!(!LivingEntity::can_equip_with_dispenser(&baby, &saddle));

        let dead = PigEntity::new(&vanilla_entities::PIG, 3, DVec3::ZERO, Weak::new());
        dead.set_health(0.0);
        assert!(!LivingEntity::can_equip_with_dispenser(&dead, &saddle));

        let unequippable_target =
            PigEntity::new(&vanilla_entities::PIG, 4, DVec3::ZERO, Weak::new());
        let stone = ItemStack::new(&ITEMS.stone);
        assert!(!LivingEntity::can_equip_with_dispenser(
            &unequippable_target,
            &stone
        ));
    }

    #[test]
    fn pig_living_is_baby_uses_ageable_state() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!LivingEntity::is_baby(&pig));

        pig.set_baby(true);

        assert!(LivingEntity::is_baby(&pig));
    }

    #[test]
    fn pig_saddled_state_reads_saddle_equipment() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!pig.is_saddled());

        pig.living_base
            .equipment()
            .lock()
            .set(EquipmentSlot::Saddle, ItemStack::new(&ITEMS.saddle));

        assert!(pig.is_saddled());
    }

    #[test]
    fn pig_saddle_equip_sound_uses_vanilla_sound() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let saddle = ItemStack::new(&ITEMS.saddle);

        assert_eq!(
            LivingEntity::equip_sound(&pig, EquipmentSlot::Saddle, &saddle)
                .map(|sound| sound.key.to_string()),
            Some("minecraft:entity.pig.saddle".to_owned())
        );
        assert!(LivingEntity::equip_sound(&pig, EquipmentSlot::Head, &saddle).is_none());
    }

    #[test]
    fn pig_hurt_and_death_sounds_use_current_sound_variant() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert_eq!(
            LivingEntity::hurt_sound(&pig, &source).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_PIG_HURT.key)
        );

        pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);
        assert_eq!(
            LivingEntity::death_sound(&pig).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_PIG_BIG_DEATH.key)
        );

        pig.set_baby(true);
        assert_eq!(
            LivingEntity::hurt_sound(&pig, &source).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_BABY_PIG_HURT.key)
        );
    }

    #[test]
    fn pig_ambient_sound_uses_current_sound_variant() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        assert_eq!(Mob::ambient_sound_interval(&pig), 120);

        assert_eq!(
            Mob::ambient_sound(&pig).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_PIG_AMBIENT.key)
        );

        pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);
        assert_eq!(
            Mob::ambient_sound(&pig).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_PIG_BIG_AMBIENT.key)
        );

        pig.set_baby(true);
        assert_eq!(
            Mob::ambient_sound(&pig).map(|sound| &sound.key),
            Some(&sound_events::ENTITY_BABY_PIG_AMBIENT.key)
        );
    }

    #[test]
    fn pig_uses_vanilla_animal_experience_reward() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        for _ in 0..16 {
            let reward = LivingEntity::base_experience_reward(&pig);
            assert!((1..=3).contains(&reward));
        }
    }

    #[test]
    fn pig_baby_and_consumed_experience_follow_living_rules() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        assert!(LivingEntity::should_drop_experience(&pig));
        assert!(!LivingEntity::was_experience_consumed(&pig));

        LivingEntity::skip_drop_experience(&pig);
        assert!(LivingEntity::was_experience_consumed(&pig));

        pig.living_base().reset_death_state();
        assert!(!LivingEntity::was_experience_consumed(&pig));

        pig.set_baby(true);
        assert!(!LivingEntity::should_drop_experience(&pig));
    }

    #[test]
    fn mob_guaranteed_drop_marks_slot_preserved() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert_eq!(
            pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
            0.085_f32.to_bits()
        );
        assert!(!pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));

        pig.set_guaranteed_drop(EquipmentSlot::Saddle);

        assert_eq!(
            pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
            2.0_f32.to_bits()
        );
        assert!(pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));
        assert_eq!(
            pig.equipment_drop_chance(EquipmentSlot::Head).to_bits(),
            0.085_f32.to_bits()
        );
    }

    #[test]
    fn mob_death_loot_without_world_keeps_preserved_equipment() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.living_base
            .equipment()
            .lock()
            .set(EquipmentSlot::Saddle, ItemStack::new(&ITEMS.saddle));
        pig.set_guaranteed_drop(EquipmentSlot::Saddle);

        pig.drop_custom_death_loot_mob(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            false,
        );

        assert!(pig.is_saddled());
        assert!(pig.is_equipment_drop_preserved(EquipmentSlot::Saddle));
    }

    #[test]
    fn pig_breeding_offspring_inherits_parent_variant() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let partner = PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::new(1.0, 0.0, 0.0),
            Weak::new(),
        );
        let offspring = PigEntity::new(
            &vanilla_entities::PIG,
            3,
            DVec3::new(2.0, 0.0, 0.0),
            Weak::new(),
        );
        pig.set_variant(&vanilla_pig_variants::WARM);
        partner.set_variant(&vanilla_pig_variants::COLD);
        offspring.set_variant(&vanilla_pig_variants::TEMPERATE);

        pig.initialize_breed_offspring(&partner, &offspring);

        let variant_key = &offspring.variant().key;
        assert!(
            variant_key == &vanilla_pig_variants::WARM.key
                || variant_key == &vanilla_pig_variants::COLD.key
        );
    }

    #[test]
    fn pig_mob_ai_increments_no_action_time() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        pig.set_no_action_time(12);
        Mob::mob_server_ai_step(&pig);

        assert_eq!(pig.no_action_time(), 13);
    }

    #[test]
    fn pig_damage_resets_no_action_time() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        pig.set_no_action_time(42);
        assert!(pig.hurt_server(&source, 1.0));

        assert_eq!(pig.no_action_time(), 0);
    }

    #[test]
    fn pig_keeps_vanilla_animal_far_away_persistence() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert!(!pig.remove_when_far_away(f64::MAX));
    }

    #[test]
    fn pig_registers_vanilla_passive_goal_foundations() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        let selector = pig.mob_base().goal_selector().lock();
        assert_eq!(selector.available_goal_count(), 9);
        assert_eq!(
            selector.available_goal_priorities(),
            vec![0, 1, 3, 4, 4, 5, 6, 7, 8]
        );
        drop(selector);
        assert!(pig.mob_base().navigation().lock().can_float());
    }

    #[test]
    fn pig_path_target_feeds_move_control_forward_input() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let path = Path::new(vec![Node::new(1, 0, 0)], BlockPos::new(1, 0, 0), true);

        let level = EmptyNavigationLevel::new();
        assert!(
            pig.mob_base()
                .navigation()
                .lock()
                .move_to(&level, path, 1.0, pig.position())
        );
        let target = {
            let mut navigation = pig.mob_base().navigation().lock();
            navigation.next_move_target(NavigationTickContext {
                mob_position: pig.position(),
                mob_bounding_box_width: pig.bounding_box().width(),
                mob_speed: pig.get_speed(),
                game_time: 0,
            })
        };
        let Some((target, speed_modifier)) = target else {
            panic!("navigation should provide a move target");
        };

        pig.set_wanted_position(target, speed_modifier);
        Mob::tick_move_control(&pig);

        assert_eq!(pig.get_speed().to_bits(), 0.25_f32.to_bits());
        assert_eq!(pig.travel_input().forward().to_bits(), 0.25_f32.to_bits());
    }

    #[test]
    fn pig_age_updates_synchronized_baby_flag_on_boundary() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        pig.set_age(-1);
        assert!(pig.is_baby());
        assert!(*pig.entity_data.lock().ageable_mob().baby.get());

        pig.set_age(0);
        assert!(!pig.is_baby());
        assert!(!*pig.entity_data.lock().ageable_mob().baby.get());
    }

    #[test]
    fn pig_age_boundary_refreshes_dimensions() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let adult_dimensions = vanilla_entities::PIG.dimensions;

        assert_eq!(pig.base().dimensions(), adult_dimensions);

        pig.set_age(-1);
        let baby_dimensions = PIG_BABY_DIMENSIONS;
        assert_eq!(pig.base().dimensions(), baby_dimensions);
        assert_eq!(baby_dimensions.eye_height.to_bits(), 0.40625_f32.to_bits());
        assert_eq!(
            baby_dimensions
                .attachments
                .get_clamped(EntityAttachment::Passenger, 0, 0.0, baby_dimensions)
                .y
                .to_bits(),
            0.5_f64.to_bits()
        );
        assert_eq!(
            pig.bounding_box().width().to_bits(),
            f64::from(baby_dimensions.width).to_bits()
        );
        assert_eq!(
            pig.bounding_box().height().to_bits(),
            f64::from(baby_dimensions.height).to_bits()
        );

        pig.set_age(0);
        assert_eq!(pig.base().dimensions(), adult_dimensions);
        assert_eq!(
            pig.bounding_box().width().to_bits(),
            f64::from(adult_dimensions.width).to_bits()
        );
        assert_eq!(
            pig.bounding_box().height().to_bits(),
            f64::from(adult_dimensions.height).to_bits()
        );
    }

    #[test]
    fn pig_scale_attribute_refreshes_dimensions() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let adult_dimensions = vanilla_entities::PIG.dimensions;

        pig.attributes()
            .lock()
            .set_base_value(vanilla_attributes::SCALE, 2.0);
        LivingEntity::refresh_dirty_attributes(&pig);

        let scaled_dimensions = adult_dimensions.scale(2.0);
        assert_eq!(pig.base().dimensions(), scaled_dimensions);
        assert_eq!(
            pig.bounding_box().width().to_bits(),
            f64::from(scaled_dimensions.width).to_bits()
        );
        assert_eq!(
            pig.bounding_box().height().to_bits(),
            f64::from(scaled_dimensions.height).to_bits()
        );
    }

    #[test]
    fn pig_saves_vanilla_mob_age_and_variant_data() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.set_can_pick_up_loot(true);
        pig.set_persistence_required();
        pig.set_guaranteed_drop(EquipmentSlot::Saddle);
        pig.set_home_to(BlockPos::new(11, 64, -3), 7);
        pig.set_death_loot_table(Some(Identifier::vanilla_static("entities/pig")));
        pig.set_death_loot_table_seed(1234);
        let leash_holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        assert!(pig.set_leashed_to(&leash_holder));
        pig.set_no_ai(true);
        pig.set_left_handed(true);
        pig.set_age(-24_000);
        pig.set_forced_age(12);
        pig.set_age_locked(true);
        pig.set_variant(&vanilla_pig_variants::WARM);
        pig.set_sound_variant(&vanilla_pig_sound_variants::BIG);

        let mut nbt = NbtCompound::new();
        pig.save_additional(&mut nbt);

        assert_eq!(nbt.byte("CanPickUpLoot"), Some(1));
        assert_eq!(nbt.byte("PersistenceRequired"), Some(1));
        let Some(drop_chances) = nbt.compound("drop_chances") else {
            panic!("non-default mob drop chances should be saved");
        };
        assert_eq!(drop_chances.float("saddle"), Some(2.0));
        assert_eq!(drop_chances.float("head"), None);
        assert_eq!(nbt.int("home_radius"), Some(7));
        assert_eq!(
            nbt.int_array("home_pos").map(<[i32]>::to_vec),
            Some(vec![11, 64, -3])
        );
        assert_eq!(
            nbt.string("DeathLootTable").map(ToString::to_string),
            Some("minecraft:entities/pig".to_owned())
        );
        assert_eq!(nbt.long("DeathLootTableSeed"), Some(1234));
        let Some(leash) = nbt.compound("leash") else {
            panic!("live leash holder should save as a UUID compound");
        };
        assert_eq!(
            leash.int_array("UUID").map(<[i32]>::to_vec),
            Some(leash_holder.uuid().to_int_array().to_vec())
        );
        assert_eq!(nbt.byte("NoAI"), Some(1));
        assert_eq!(nbt.byte("LeftHanded"), Some(1));
        assert_eq!(nbt.int("Age"), Some(-24_000));
        assert_eq!(nbt.int("ForcedAge"), Some(12));
        assert_eq!(nbt.byte("AgeLocked"), Some(1));
        assert_eq!(
            nbt.string("variant").map(ToString::to_string),
            Some("minecraft:warm".to_owned())
        );
        assert_eq!(
            nbt.string("sound_variant").map(ToString::to_string),
            Some("minecraft:big".to_owned())
        );
    }

    #[test]
    fn pig_loads_vanilla_mob_age_and_variant_data() {
        init_test_registry();

        let mut nbt = NbtCompound::new();
        nbt.insert("CanPickUpLoot", 1_i8);
        nbt.insert("PersistenceRequired", 1_i8);
        let mut drop_chances = NbtCompound::new();
        drop_chances.insert("saddle", 2.0_f32);
        nbt.insert("drop_chances", NbtTag::Compound(drop_chances));
        nbt.insert("home_radius", 7_i32);
        nbt.insert("home_pos", NbtTag::IntArray(vec![11, 64, -3]));
        nbt.insert("DeathLootTable", "minecraft:entities/pig");
        nbt.insert("DeathLootTableSeed", 1234_i64);
        let leash_uuid = Uuid::from_u128(43);
        let mut leash = NbtCompound::new();
        leash.insert("UUID", NbtTag::IntArray(leash_uuid.to_int_array().to_vec()));
        nbt.insert("leash", NbtTag::Compound(leash));
        nbt.insert("NoAI", 1_i8);
        nbt.insert("LeftHanded", 1_i8);
        nbt.insert("Age", -24_000_i32);
        nbt.insert("ForcedAge", 12_i32);
        nbt.insert("AgeLocked", 1_i8);
        nbt.insert("variant", "minecraft:cold");
        nbt.insert("sound_variant", "minecraft:mini");

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.load_additional((&borrowed).into());

        assert!(pig.can_pick_up_loot());
        assert!(pig.is_persistence_required());
        assert_eq!(
            pig.equipment_drop_chance(EquipmentSlot::Saddle).to_bits(),
            2.0_f32.to_bits()
        );
        assert_eq!(
            pig.equipment_drop_chance(EquipmentSlot::Head).to_bits(),
            0.085_f32.to_bits()
        );
        assert!(pig.has_home());
        assert_eq!(pig.home_radius(), 7);
        assert_eq!(pig.home_position(), BlockPos::new(11, 64, -3));
        let mut saved = NbtCompound::new();
        pig.save_additional(&mut saved);
        assert_eq!(
            saved.string("DeathLootTable").map(ToString::to_string),
            Some("minecraft:entities/pig".to_owned())
        );
        assert_eq!(saved.long("DeathLootTableSeed"), Some(1234));
        assert!(pig.may_be_leashed());
        assert!(!pig.is_leashed());
        assert_eq!(
            pig.leash_attachment(),
            Some(LeashAttachment::Entity(leash_uuid))
        );
        assert!(pig.is_no_ai());
        assert!(pig.is_left_handed());
        assert_eq!(pig.get_age(), -24_000);
        assert_eq!(pig.forced_age(), 12);
        assert!(pig.is_age_locked());
        assert_eq!(pig.variant().key, vanilla_pig_variants::COLD.key);
        assert_eq!(
            pig.sound_variant().key,
            vanilla_pig_sound_variants::MINI.key
        );
    }

    #[test]
    fn pig_saves_delayed_fence_knot_leash_as_vanilla_block_pos_int_array() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.set_delayed_leash_attachment(LeashAttachment::FenceKnot(BlockPos::new(4, 65, -9)));

        let mut nbt = NbtCompound::new();
        pig.save_additional(&mut nbt);

        assert_eq!(
            nbt.int_array("leash").map(<[i32]>::to_vec),
            Some(vec![4, 65, -9])
        );
    }

    #[test]
    fn pig_saves_live_fence_knot_leash_as_vanilla_block_pos_int_array() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let knot: SharedEntity = Arc::new(LeashFenceKnotEntity::new_attached(
            &vanilla_entities::LEASH_KNOT,
            2,
            BlockPos::new(4, 65, -9),
            Weak::new(),
        ));
        assert!(pig.set_leashed_to(&knot));

        let mut nbt = NbtCompound::new();
        pig.save_additional(&mut nbt);

        assert_eq!(
            nbt.int_array("leash").map(<[i32]>::to_vec),
            Some(vec![4, 65, -9])
        );
    }

    #[test]
    fn pig_loads_delayed_fence_knot_leash_from_vanilla_block_pos_int_array() {
        init_test_registry();

        let mut nbt = NbtCompound::new();
        nbt.insert("leash", NbtTag::IntArray(vec![4, 65, -9]));

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.load_additional((&borrowed).into());

        assert!(pig.may_be_leashed());
        assert!(!pig.is_leashed());
        assert_eq!(
            pig.leash_attachment(),
            Some(LeashAttachment::FenceKnot(BlockPos::new(4, 65, -9)))
        );
    }

    #[test]
    fn pig_drop_leash_clears_live_leash_state() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        assert!(pig.set_leashed_to(&holder));

        pig.drop_leash();

        assert!(!pig.is_leashed());
        assert!(!pig.may_be_leashed());
    }

    #[test]
    fn pig_remove_leash_clears_live_leash_state() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        assert!(pig.set_leashed_to(&holder));

        pig.remove_leash();

        assert!(!pig.is_leashed());
        assert!(!pig.may_be_leashed());
    }

    #[test]
    fn pig_drop_all_leash_connections_clears_own_live_leash() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let holder: SharedEntity = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        assert!(pig.set_leashed_to(&holder));

        assert!(pig.drop_all_leash_connections(None));

        assert!(!pig.is_leashed());
        assert!(!pig.may_be_leashed());
    }

    #[test]
    fn pig_uses_vanilla_animal_fire_path_malus() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());

        assert_eq!(
            pig.get_pathfinding_malus(PathType::FireInNeighbor)
                .to_bits(),
            16.0_f32.to_bits()
        );
        assert_eq!(
            pig.get_pathfinding_malus(PathType::Fire).to_bits(),
            (-1.0_f32).to_bits()
        );
    }

    #[test]
    fn pig_uses_mob_passenger_as_controller_when_not_player_controlled() {
        init_test_registry();

        let vehicle_pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            1,
            DVec3::ZERO,
            Weak::new(),
        ));
        let vehicle: SharedEntity = vehicle_pig.clone();
        let passenger_pig = Arc::new(PigEntity::new(
            &vanilla_entities::PIG,
            2,
            DVec3::ZERO,
            Weak::new(),
        ));
        let passenger: SharedEntity = passenger_pig.clone();
        EntityBase::restore_passenger_relationship(&vehicle, &passenger);

        assert_eq!(
            vehicle
                .controlling_passenger()
                .map(|controller| controller.id()),
            Some(passenger.id())
        );

        passenger_pig.set_wanted_position(DVec3::new(1.0, 0.0, 0.0), 1.0);
        Mob::tick_move_control(vehicle_pig.as_ref());

        assert_eq!(vehicle_pig.get_speed().to_bits(), 0.25_f32.to_bits());
        assert_eq!(
            vehicle_pig.travel_input().forward().to_bits(),
            0.25_f32.to_bits()
        );
        Mob::tick_move_control(passenger_pig.as_ref());
        assert_eq!(
            passenger_pig.travel_input().forward().to_bits(),
            0.0_f32.to_bits()
        );
    }

    #[test]
    fn pig_uses_vanilla_pig_food_tag() {
        init_test_registry();

        assert!(PigEntity::is_food(&ItemStack::new(&ITEMS.carrot)));
        assert!(!PigEntity::is_food(&ItemStack::new(&ITEMS.stone)));
    }

    #[test]
    fn pig_saves_vanilla_animal_love_data() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let love_cause = Uuid::from_u128(42);
        pig.set_in_love_time(123);
        pig.set_love_cause_uuid(Some(love_cause));

        let mut nbt = NbtCompound::new();
        pig.save_additional(&mut nbt);

        assert_eq!(nbt.int("InLove"), Some(123));
        assert_eq!(
            nbt.int_array("LoveCause").map(<[i32]>::to_vec),
            Some(love_cause.to_int_array().to_vec())
        );
    }

    #[test]
    fn pig_loads_vanilla_animal_love_data() {
        init_test_registry();

        let love_cause = Uuid::from_u128(42);
        let mut nbt = NbtCompound::new();
        nbt.insert("InLove", 321_i32);
        nbt.insert(
            "LoveCause",
            NbtTag::IntArray(love_cause.to_int_array().to_vec()),
        );

        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_borrowed_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("test nbt should reborrow: {error}"));

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.load_additional((&borrowed).into());

        assert_eq!(pig.in_love_time(), 321);
        assert_eq!(pig.love_cause_uuid(), Some(love_cause));
    }

    #[test]
    fn pig_animal_love_ticks_only_for_adults() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.set_in_love_time(2);
        Animal::tick_animal_love(&pig);
        assert_eq!(pig.in_love_time(), 1);

        pig.set_age(-1);
        pig.set_in_love_time(20);
        Animal::tick_animal_love(&pig);
        assert_eq!(pig.in_love_time(), 0);
    }

    #[test]
    fn pig_damage_resets_vanilla_animal_love_time() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);
        pig.set_in_love_time(20);

        assert!(pig.hurt_server(&source, 1.0));

        assert_eq!(pig.in_love_time(), 0);
    }

    #[test]
    fn pig_death_tick_removes_after_vanilla_death_duration() {
        init_test_registry();

        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        pig.set_health(0.0);

        for _ in 0..DEATH_DURATION {
            pig.tick();
        }

        assert_eq!(pig.removal_reason(), Some(RemovalReason::Killed));
    }
}
