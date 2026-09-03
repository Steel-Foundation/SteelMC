//! Vanilla Nautilus: aquatic tamable mount (`AbstractNautilus`).

use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::NbtCompound;
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::FOOD;
use steel_registry::entity_type::{
    EntityAttachmentPoint, EntityAttachments, EntityDimensions, EntityTypeRef,
};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::NautilusEntityData;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, sound_events, vanilla_attributes, vanilla_mob_effects,
};
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};
use uuid::Uuid;

use crate::behavior::InteractionResult;
use crate::entity::ai::goal::{
    BreedGoal, FollowOwnerGoal, FollowParentGoal, LookAtPlayerGoal, PanicGoal,
    RandomLookAroundGoal, RandomSwimmingGoal, TemptGoal, TryFindWaterGoal,
};
use crate::entity::damage::DamageSource;
use crate::entity::mob::{fish_init_mob_base, fish_tick_move_control, fish_travel};
use crate::entity::{
    AgeableMob, AgeableMobBase, Animal, AnimalBase, Entity, EntityBase, EntityBaseLoad, EntityPose,
    EntitySpawnReason, EntitySyncedData, LivingEntity, LivingEntityBase, LivingEntitySyncedData,
    Mob, MobBase, MobEffectInstance, PathfinderMob, SharedEntity, SpawnGroupData, TamableAnimal,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

const NAUTILUS_BABY_PASSENGER_ATTACHMENTS: [EntityAttachmentPoint; 1] =
    [EntityAttachmentPoint::new(0.0, 0.56875, 0.0)];
const NAUTILUS_BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new_with_attachments(
    0.4375,
    0.475,
    0.13755,
    EntityAttachments::new(&NAUTILUS_BABY_PASSENGER_ATTACHMENTS, &[], &[], &[]),
);
const TAMING_CHANCE_BOUND: i32 = 3;
const RIDDEN_SPEED_SCALE: f32 = 0.225;
const BREATH_OF_THE_NAUTILUS_DURATION: i32 = 60;

#[entity_behavior(class = "Nautilus")]
/// Entity behavior for the nautilus.
pub struct NautilusEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    mob_base: MobBase,
    ageable_base: AgeableMobBase,
    animal_base: AnimalBase,
    entity_data: SyncMutex<NautilusEntityData>,
}

// SAFETY: key is owner-scoped to this Steel entity type; the implementation only
// exposes `NautilusEntity` itself.
unsafe impl DowncastType for NautilusEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/nautilus");
}

impl NautilusEntity {
    #[must_use]
    /// Creates a new instance.
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    #[must_use]
    /// Creates an instance from saved data.
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
        fish_init_mob_base(&mob_base);
        let mut entity_data = NautilusEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        {
            let mut goal_selector = mob_base.goal_selector().lock();
            goal_selector.add_goal(0, TryFindWaterGoal::new());
            goal_selector.add_goal(1, PanicGoal::new(1.25));
            goal_selector.add_goal(
                2,
                TemptGoal::new(
                    1.25,
                    |item_stack| NautilusEntity::is_food(item_stack),
                    false,
                ),
            );
            goal_selector.add_goal(3, BreedGoal::new(1.0));
            goal_selector.add_goal(4, FollowOwnerGoal::new(1.0));
            goal_selector.add_goal(5, FollowParentGoal::new(1.1));
            goal_selector.add_goal(6, RandomSwimmingGoal::new(1.0, 40));
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
            entity_data: SyncMutex::new(entity_data),
        }
    }

    /// Returns whether the stack is vanilla nautilus food (`#minecraft:nautilus_food`).
    #[must_use]
    pub fn is_food(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::NAUTILUS_FOOD)
    }

    /// Returns whether the stack can tame a wild nautilus (`#minecraft:nautilus_taming_items`).
    #[must_use]
    pub fn is_taming_item(item_stack: &ItemStack) -> bool {
        REGISTRY
            .items
            .is_in_tag(item_stack.item(), &ItemTag::NAUTILUS_TAMING_ITEMS)
    }

    fn set_ridden_rotation(&self, controller_yaw: f32, controller_pitch: f32) {
        self.set_rotation((controller_yaw, controller_pitch * 0.5));
        self.base.set_old_yaw_to_current();
        let yaw = self.rotation().0;
        self.set_y_body_rot(yaw);
        self.set_y_head_rot(yaw);
    }

    fn play_eating_sound_nautilus(&self) {
        self.play_sound(&sound_events::ENTITY_NAUTILUS_EAT, 1.0, 1.0);
    }

    fn try_tame(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        Mob::use_player_item(self, player, hand);
        self.play_eating_sound_nautilus();
        if rand::random_range(0..TAMING_CHANCE_BOUND) == 0 {
            self.tame(player);
        } else {
            self.broadcast_entity_event(steel_utils::entity_events::EntityStatus::TamingFailed);
        }
        InteractionResult::SuccessServer
    }

    fn try_heal_with_food(
        &self,
        player: &Player,
        hand: InteractionHand,
        stack: &ItemStack,
    ) -> bool {
        if self.get_health() >= self.get_max_health() {
            return false;
        }
        let nutrition = stack
            .get(FOOD)
            .map(steel_registry::data_components::components::FoodProperties::nutrition)
            .unwrap_or(1);
        if nutrition <= 0 {
            return false;
        }
        Mob::use_player_item(self, player, hand);
        self.heal(nutrition as f32);
        self.play_eating_sound_nautilus();
        true
    }

    fn update_dirty_mob_effect_entity_data(&self) {
        if !self.living_base.take_effects_dirty() {
            return;
        }
        let display = self.living_base.mob_effect_display_state();
        {
            let mut d = self.entity_data.lock();
            let l = d.living_entity_mut();
            l.effect_particles.set(display.particles);
            l.effect_ambience.set(display.ambient);
        }
        self.entity_data.set_base_invisible_flag(display.invisible);
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    fn in_water_sound(&self, water: SoundEventRef, land: SoundEventRef) -> SoundEventRef {
        if self.is_in_water() { water } else { land }
    }
}

impl Entity for NautilusEntity {
    fn controlling_passenger(&self) -> Option<SharedEntity> {
        super::controlling_passenger_mountable(self, Mob::is_saddled(self) && self.is_tame())
    }

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
            NAUTILUS_BABY_DIMENSIONS.scale(scale)
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

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn play_step_sound(&self, _pos: BlockPos, _block: BlockStateId) {
        self.play_sound(
            self.in_water_sound(
                &sound_events::ENTITY_NAUTILUS_SWIM,
                &sound_events::ENTITY_NAUTILUS_AMBIENT_LAND,
            ),
            0.15,
            1.0,
        );
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.save_mob(nbt);
        self.save_ageable_mob(nbt);
        self.save_animal(nbt);
        self.save_tamable(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.load_mob(nbt);
        self.load_ageable_mob(nbt);
        self.load_animal(nbt);
        self.load_tamable(nbt);
    }
}

impl LivingEntity for NautilusEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn living_synced_data(&self) -> Option<&dyn LivingEntitySyncedData> {
        Some(&self.entity_data)
    }

    fn get_health(&self) -> f32 {
        *self.entity_data.lock().living_entity().health.get()
    }

    fn set_health(&self, h: f32) {
        let m = self.get_max_health();
        let c = h.clamp(0.0, m);
        self.entity_data.lock().living_entity_mut().health.set(c);
    }

    fn sound_volume(&self) -> f32 {
        0.4
    }

    fn hurt_sound(&self, _s: &DamageSource) -> Option<SoundEventRef> {
        Some(self.in_water_sound(
            &sound_events::ENTITY_NAUTILUS_HURT,
            &sound_events::ENTITY_NAUTILUS_HURT_LAND,
        ))
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(self.in_water_sound(
            &sound_events::ENTITY_NAUTILUS_DEATH,
            &sound_events::ENTITY_NAUTILUS_DEATH_LAND,
        ))
    }

    fn can_use_slot(&self, slot: EquipmentSlot) -> bool {
        match slot {
            EquipmentSlot::Saddle | EquipmentSlot::Body => {
                Entity::is_alive(self) && !AgeableMob::is_baby(self) && self.is_tame()
            }
            _ => true,
        }
    }

    fn can_dispenser_equip_into_slot(&self, slot: EquipmentSlot) -> bool {
        matches!(slot, EquipmentSlot::Saddle | EquipmentSlot::Body) || Mob::can_pick_up_loot(self)
    }

    fn server_ai_step(&self) {
        Mob::mob_server_ai_step(self);
    }

    fn tick_ridden(&self, controller: &Player, _ridden_input: DVec3) {
        let (yaw, pitch) = controller.rotation();
        self.set_ridden_rotation(yaw, pitch);
        controller.add_mob_effect(
            MobEffectInstance::with_duration(
                vanilla_mob_effects::BREATH_OF_THE_NAUTILUS,
                BREATH_OF_THE_NAUTILUS_DURATION,
                0,
            )
            .with_ambient(true)
            .with_visible(false),
        );
    }

    fn ridden_input(&self, controller: &Player, _self_input: DVec3) -> DVec3 {
        let input = controller.travel_input();
        let forward = if input.forward() <= 0.0 {
            input.forward() * 0.25
        } else {
            input.forward()
        };
        DVec3::new(
            f64::from(input.sideways()) * 0.5,
            f64::from(input.vertical()),
            f64::from(forward),
        )
    }

    fn ridden_speed(&self, _controller: &Player) -> f32 {
        let movement_speed = self
            .attributes()
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED) as f32;
        movement_speed * RIDDEN_SPEED_SCALE
    }

    fn travel_in_water(
        &self,
        input: DVec3,
        _base_gravity: f64,
        _is_falling: bool,
        _old_y: f64,
    ) -> Option<MoveResult> {
        fish_travel(self, input)
    }

    fn ai_step(&self) -> Option<MoveResult> {
        if !self.is_in_water() && self.on_ground() && self.vertical_collision() {
            let mut vel = self.velocity();
            vel.x += (rand::random::<f64>() - 0.5) * 0.05;
            vel.y += 0.4;
            vel.z += (rand::random::<f64>() - 0.5) * 0.05;
            self.set_velocity(vel);
            self.set_on_ground(false);
            self.play_sound(&sound_events::ENTITY_NAUTILUS_AMBIENT_LAND, 1.0, 1.0);
        }
        let result = self.default_ai_step();
        AgeableMob::tick_ageable_mob(self);
        Animal::tick_animal_love(self);
        result
    }
}

impl AgeableMob for NautilusEntity {
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

impl Animal for NautilusEntity {
    fn animal_base(&self) -> &AnimalBase {
        &self.animal_base
    }

    fn is_food(&self, item_stack: &ItemStack) -> bool {
        NautilusEntity::is_food(item_stack)
    }

    fn can_fall_in_love(&self) -> bool {
        self.is_tame() && self.in_love_time() <= 0
    }

    fn play_eating_sound(&self) {
        self.play_eating_sound_nautilus();
    }
}

impl TamableAnimal for NautilusEntity {
    fn tamable_flags(&self) -> i8 {
        *self.entity_data.lock().tamable_animal().flags.get()
    }

    fn set_tamable_flags(&self, flags: i8) {
        self.entity_data
            .lock()
            .tamable_animal_mut()
            .flags
            .set(flags);
    }

    fn owner_uuid(&self) -> Option<Uuid> {
        *self.entity_data.lock().tamable_animal().owneruuid.get()
    }

    fn set_owner_uuid(&self, owner: Option<Uuid>) {
        self.entity_data
            .lock()
            .tamable_animal_mut()
            .owneruuid
            .set(owner);
    }
}

impl Mob for NautilusEntity {
    fn mob_base(&self) -> &MobBase {
        &self.mob_base
    }

    fn tick_goal_selectors(&self) {
        PathfinderMob::tick_pathfinder_goal_selectors(self);
    }

    fn tick_path_navigation(&self) {
        PathfinderMob::tick_pathfinder_path_navigation(self);
    }

    fn tick_move_control(&self) {
        fish_tick_move_control(self);
    }

    fn custom_server_ai_step(&self) {
        Animal::custom_server_ai_step_animal(self);
    }

    fn ambient_sound(&self) -> Option<SoundEventRef> {
        Some(self.in_water_sound(
            &sound_events::ENTITY_NAUTILUS_AMBIENT,
            &sound_events::ENTITY_NAUTILUS_AMBIENT_LAND,
        ))
    }

    fn finalize_spawn(
        &self,
        world: &Arc<World>,
        r: EntitySpawnReason,
        g: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        self.finalize_spawn_ageable_mob(world, r, g)
    }

    fn mob_flags(&self) -> i8 {
        *self.entity_data.lock().mob().mob_flags.get()
    }

    fn set_mob_flags(&self, f: i8) {
        self.entity_data.lock().mob_mut().mob_flags.set(f);
    }

    fn mob_interact(&self, player: &Player, hand: InteractionHand) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            let item_stack = inventory.get_item_in_hand(hand);
            item_stack.copy_with_count(item_stack.count())
        };

        if Self::is_taming_item(&item_stack) && !self.is_tame() && !AgeableMob::is_baby(self) {
            return self.try_tame(player, hand);
        }

        if self.is_tame()
            && Self::is_food(&item_stack)
            && self.try_heal_with_food(player, hand, &item_stack)
        {
            return InteractionResult::SuccessServer;
        }

        let animal_result = Animal::mob_interact_animal(self, player, hand);
        if animal_result.consumes_action() {
            return animal_result;
        }

        if self.is_tame()
            && LivingEntity::is_equippable_in_slot(self, &item_stack, EquipmentSlot::Saddle)
        {
            return LivingEntity::interact_living_entity_with_equippable(self, player, hand);
        }
        if self.is_tame()
            && LivingEntity::is_equippable_in_slot(self, &item_stack, EquipmentSlot::Body)
        {
            return LivingEntity::interact_living_entity_with_equippable(self, player, hand);
        }

        if self.is_tame()
            && self.is_saddled()
            && !self.is_vehicle()
            && !player.is_secondary_use_active()
            && !AgeableMob::is_baby(self)
        {
            if let Some(world) = self.level()
                && let Some(vehicle) = world.get_entity_by_id(self.id())
            {
                player.start_riding(&vehicle);
            }
            return InteractionResult::Success;
        }

        InteractionResult::Pass
    }
}

impl PathfinderMob for NautilusEntity {}

#[cfg(test)]
mod tests;
