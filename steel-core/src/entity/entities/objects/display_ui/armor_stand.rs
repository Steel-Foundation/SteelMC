//! Armor stand entity (`ArmorStand`).

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::{NbtCompound as BorrowedNbtCompoundView, NbtTag as BorrowedNbtTag};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::{CEntityEvent, SoundSource};
use steel_registry::blocks::behavior::PushReaction;
use steel_registry::data_components::vanilla_components::CUSTOM_NAME;
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::entity_data::{EntityPose, Rotations};
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{BlockParticleOption, ParticleData};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_damage_type_tags::DamageTypeTag;
use steel_registry::vanilla_entity_data::ArmorStandEntityData;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::{
    sound_events, vanilla_attributes, vanilla_blocks, vanilla_entities, vanilla_game_events,
    vanilla_items, vanilla_particle_types,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::enchantment_helper;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntitySyncedData, LivingEntity, LivingEntityBase,
    RemovalReason,
};
use crate::inventory::equipment::{EquipmentSlot, EquipmentSlotType};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

/// Time in ticks for hit wobble animation cooldown.
pub const WOBBLE_TIME: i64 = 5;

/// Client flag bit for small / baby armor stands.
pub const CLIENT_FLAG_SMALL: i8 = 1;
/// Client flag bit for showing arms on armor stands.
pub const CLIENT_FLAG_SHOW_ARMS: i8 = 4;
/// Client flag bit for hiding the armor stand baseplate.
pub const CLIENT_FLAG_NO_BASEPLATE: i8 = 8;
/// Client flag bit for marker mode (no dimensions, no physics).
pub const CLIENT_FLAG_MARKER: i8 = 16;

/// Default head pose rotations (0, 0, 0).
pub const DEFAULT_HEAD_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
/// Default body pose rotations (0, 0, 0).
pub const DEFAULT_BODY_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
/// Default left arm pose rotations (-10, 0, -10).
pub const DEFAULT_LEFT_ARM_POSE: Rotations = Rotations::new(-10.0, 0.0, -10.0);
/// Default right arm pose rotations (-15, 0, 10).
pub const DEFAULT_RIGHT_ARM_POSE: Rotations = Rotations::new(-15.0, 0.0, 10.0);
/// Default left leg pose rotations (-1, 0, -1).
pub const DEFAULT_LEFT_LEG_POSE: Rotations = Rotations::new(-1.0, 0.0, -1.0);
/// Default right leg pose rotations (1, 0, 1).
pub const DEFAULT_RIGHT_LEG_POSE: Rotations = Rotations::new(1.0, 0.0, 1.0);

const MARKER_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.0, 0.0, 0.0);
const BABY_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.25, 0.9875, 0.9875);

fn nbt_bool(value: bool) -> NbtTag {
    NbtTag::Byte(i8::from(value))
}

fn save_rotations(rotations: Rotations) -> NbtTag {
    NbtTag::List(NbtList::Float(vec![rotations.x, rotations.y, rotations.z]))
}

fn load_rotations(tag: Option<BorrowedNbtTag<'_, '_>>) -> Option<Rotations> {
    let list = tag?.list()?.floats()?;
    if list.len() < 3 {
        return None;
    }
    Some(Rotations::new(list[0], list[1], list[2]))
}

const fn set_bit(data: i8, bit: i8, value: bool) -> i8 {
    if value { data | bit } else { data & !bit }
}

/// Vanilla armor stand entity for equipment displays and marker entities.
#[entity_behavior(class = "ArmorStand")]
pub struct ArmorStandEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    entity_data: SyncMutex<ArmorStandEntityData>,
    invisible: SyncMutex<bool>,
    disabled_slots: SyncMutex<i32>,
    last_hit: SyncMutex<Option<i64>>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ArmorStandEntity`.
unsafe impl DowncastType for ArmorStandEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/armor_stand");
}

impl ArmorStandEntity {
    /// Creates a new armor stand at runtime.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Reconstructs an armor stand from persistent base entity state.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        living_base
            .attributes()
            .lock()
            .set_base_value(vanilla_attributes::STEP_HEIGHT, 0.0);

        let armor_stand = Self {
            base,
            entity_type,
            living_base,
            entity_data: SyncMutex::new(ArmorStandEntityData::new()),
            invisible: SyncMutex::new(false),
            disabled_slots: SyncMutex::new(0),
            last_hit: SyncMutex::new(None),
        };
        armor_stand.set_health(armor_stand.get_max_health());
        armor_stand
    }

    /// Sets whether this armor stand is invisible.
    pub fn set_invisible(&self, invisible: bool) {
        *self.invisible.lock() = invisible;
        self.entity_data.set_base_invisible_flag(invisible);
    }

    /// Returns whether this armor stand is small (baby size).
    #[must_use]
    pub fn is_small(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_SMALL) != 0
    }

    /// Sets whether this armor stand is small.
    pub fn set_small(&self, value: bool) {
        self.update_client_flags(CLIENT_FLAG_SMALL, value);
        self.refresh_dimensions();
    }

    /// Returns whether arms are shown on this armor stand.
    #[must_use]
    pub fn show_arms(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_SHOW_ARMS) != 0
    }

    /// Sets whether arms are shown.
    pub fn set_show_arms(&self, value: bool) {
        self.update_client_flags(CLIENT_FLAG_SHOW_ARMS, value);
    }

    /// Returns whether the baseplate is shown.
    #[must_use]
    pub fn show_base_plate(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_NO_BASEPLATE) == 0
    }

    /// Sets whether the baseplate is hidden.
    pub fn set_no_base_plate(&self, value: bool) {
        self.update_client_flags(CLIENT_FLAG_NO_BASEPLATE, value);
    }

    /// Returns whether this armor stand is a marker entity (0 dimension, no physics).
    #[must_use]
    pub fn is_marker(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_MARKER) != 0
    }

    /// Sets whether this armor stand is a marker entity.
    pub fn set_marker(&self, value: bool) {
        self.update_client_flags(CLIENT_FLAG_MARKER, value);
        self.refresh_dimensions();
    }

    /// Returns the raw client flags byte.
    #[must_use]
    pub fn client_flags(&self) -> i8 {
        *self.entity_data.lock().armor_stand().client_flags.get()
    }

    fn update_client_flags(&self, bit: i8, value: bool) {
        let mut entity_data = self.entity_data.lock();
        let current = *entity_data.armor_stand().client_flags.get();
        entity_data
            .armor_stand_mut()
            .client_flags
            .set(set_bit(current, bit, value));
    }

    /// Returns the head rotation pose.
    #[must_use]
    pub fn head_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().head_pose.get()
    }

    /// Sets the head rotation pose.
    pub fn set_head_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .head_pose
            .set(pose);
    }

    /// Returns the body rotation pose.
    #[must_use]
    pub fn body_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().body_pose.get()
    }

    /// Sets the body rotation pose.
    pub fn set_body_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .body_pose
            .set(pose);
    }

    /// Returns the left arm rotation pose.
    #[must_use]
    pub fn left_arm_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().left_arm_pose.get()
    }

    /// Sets the left arm rotation pose.
    pub fn set_left_arm_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .left_arm_pose
            .set(pose);
    }

    /// Returns the right arm rotation pose.
    #[must_use]
    pub fn right_arm_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().right_arm_pose.get()
    }

    /// Sets the right arm rotation pose.
    pub fn set_right_arm_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .right_arm_pose
            .set(pose);
    }

    /// Returns the left leg rotation pose.
    #[must_use]
    pub fn left_leg_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().left_leg_pose.get()
    }

    /// Sets the left leg rotation pose.
    pub fn set_left_leg_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .left_leg_pose
            .set(pose);
    }

    /// Returns the right leg rotation pose.
    #[must_use]
    pub fn right_leg_pose(&self) -> Rotations {
        *self.entity_data.lock().armor_stand().right_leg_pose.get()
    }

    /// Sets the right leg rotation pose.
    pub fn set_right_leg_pose(&self, pose: Rotations) {
        self.entity_data
            .lock()
            .armor_stand_mut()
            .right_leg_pose
            .set(pose);
    }

    /// Returns the disabled slots bitmask.
    #[must_use]
    pub fn disabled_slots(&self) -> i32 {
        *self.disabled_slots.lock()
    }

    /// Sets the disabled slots bitmask.
    pub fn set_disabled_slots(&self, disabled_slots: i32) {
        *self.disabled_slots.lock() = disabled_slots;
    }

    /// Returns whether a specific equipment slot is disabled.
    #[must_use]
    pub fn is_disabled(&self, slot: EquipmentSlot) -> bool {
        (self.disabled_slots() & (1 << slot.id())) != 0
            || (slot.slot_type() == EquipmentSlotType::Hand && !self.show_arms())
    }

    /// Returns the slot an item wants to occupy on this armor stand.
    #[must_use]
    pub fn get_equipment_slot_for_item(&self, item_stack: &ItemStack) -> EquipmentSlot {
        match item_stack.get_equippable() {
            Some(equippable) if self.can_use_slot(equippable.slot) => equippable.slot,
            _ => EquipmentSlot::MainHand,
        }
    }

    /// Returns whether physics and gravity apply to this armor stand.
    #[must_use]
    pub fn has_physics(&self) -> bool {
        !self.is_marker() && !self.is_no_gravity()
    }

    /// Returns the equipment slot corresponding to the hit coordinate on the armor stand.
    #[must_use]
    pub fn get_clicked_slot(&self, location: DVec3) -> EquipmentSlot {
        let mut slot_clicked = EquipmentSlot::MainHand;
        let small = self.is_small();
        let click_y = location.y / (f64::from(self.get_scale()) * f64::from(self.get_age_scale()));
        if click_y >= 0.1
            && click_y < 0.1 + (if small { 0.8 } else { 0.45 })
            && self.has_item_in_slot(EquipmentSlot::Feet)
        {
            slot_clicked = EquipmentSlot::Feet;
        } else if click_y >= 0.9 + (if small { 0.3 } else { 0.0 })
            && click_y < 0.9 + (if small { 1.0 } else { 0.7 })
            && self.has_item_in_slot(EquipmentSlot::Chest)
        {
            slot_clicked = EquipmentSlot::Chest;
        } else if click_y >= 0.4
            && click_y < 0.4 + (if small { 1.0 } else { 0.8 })
            && self.has_item_in_slot(EquipmentSlot::Legs)
        {
            slot_clicked = EquipmentSlot::Legs;
        } else if click_y >= 1.6 && self.has_item_in_slot(EquipmentSlot::Head) {
            slot_clicked = EquipmentSlot::Head;
        } else if !self.has_item_in_slot(EquipmentSlot::MainHand)
            && self.has_item_in_slot(EquipmentSlot::OffHand)
        {
            slot_clicked = EquipmentSlot::OffHand;
        }
        slot_clicked
    }

    fn swap_item(
        &self,
        player: &Player,
        slot: EquipmentSlot,
        player_item_stack: &ItemStack,
        hand: InteractionHand,
    ) -> bool {
        let item_stack = self.living_base.equipment().lock().get_ref(slot).clone();
        if !item_stack.is_empty() && (self.disabled_slots() & (1 << (slot.id() + 8))) != 0 {
            return false;
        }
        if item_stack.is_empty() && (self.disabled_slots() & (1 << (slot.id() + 16))) != 0 {
            return false;
        }

        if player.has_infinite_materials() && item_stack.is_empty() && !player_item_stack.is_empty()
        {
            self.living_base
                .equipment()
                .lock()
                .set(slot, player_item_stack.copy_with_count(1));
            return true;
        }

        if player_item_stack.is_empty() || player_item_stack.count() <= 1 {
            self.living_base
                .equipment()
                .lock()
                .set(slot, player_item_stack.clone());
            player.inventory.lock().set_item_in_hand(hand, item_stack);
            return true;
        }

        if !item_stack.is_empty() {
            return false;
        }

        let mut remaining = player_item_stack.clone();
        let placed = remaining.split(1);
        self.living_base.equipment().lock().set(slot, placed);
        player.inventory.lock().set_item_in_hand(hand, remaining);
        true
    }

    fn play_broken_sound(&self) {
        self.play_sound(&sound_events::ENTITY_ARMOR_STAND_BREAK, 1.0, 1.0);
    }

    fn show_breaking_particles(&self) {
        if let Some(world) = self.level() {
            let pos = self.position();
            let dims = self.base().dimensions();
            let height = f64::from(dims.height);
            let width = f64::from(dims.width);
            world.send_particles(
                ParticleData::new(
                    &vanilla_particle_types::BLOCK,
                    BlockParticleOption::new(vanilla_blocks::OAK_PLANKS.default_state()),
                ),
                DVec3::new(pos.x, pos.y + height * (2.0 / 3.0), pos.z),
                10,
                DVec3::new(width / 4.0, height / 4.0, width / 4.0),
                0.05,
            );
        }
    }

    fn broken_by_player(&self, source: &DamageSource, world: &World) {
        let mut result = ItemStack::new(&vanilla_items::ARMOR_STAND);
        if let Some(custom_name) = self.custom_name() {
            result.set(CUSTOM_NAME, custom_name);
        }
        let _ = self.spawn_at_location(result, 0.0);
        self.broken_by_anything(source, world);
    }

    fn broken_by_anything(&self, source: &DamageSource, _world: &World) {
        self.play_broken_sound();
        self.drop_all_death_loot(source);

        let mut equipment = self.living_base.equipment().lock();
        for slot in EquipmentSlot::ALL {
            let item_stack = equipment.take(slot);
            if !item_stack.is_empty()
                && !enchantment_helper::has_component(
                    &item_stack,
                    EnchantmentEffectComponent::PreventEquipmentDrop,
                )
            {
                let _ = self.spawn_at_location_with_offset(item_stack, DVec3::new(0.0, 1.0, 0.0));
            }
        }
    }

    fn cause_damage(&self, source: &DamageSource, damage: f32, world: &World) {
        let mut health = self.get_health();
        health -= damage;
        if health <= 0.5 {
            self.broken_by_anything(source, world);
            self.kill(world);
        } else {
            self.set_health(health);
            self.game_event(&vanilla_game_events::ENTITY_DAMAGE);
        }
    }
}

impl Entity for ArmorStandEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        self.base_tick_living_entity();
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        if self.is_marker() {
            MARKER_DIMENSIONS
        } else if self.is_small() {
            BABY_DIMENSIONS
        } else {
            self.entity_type.dimensions
        }
    }

    fn synced_data(&self) -> Option<&dyn EntitySyncedData> {
        Some(&self.entity_data)
    }

    fn max_up_step(&self) -> f32 {
        0.0
    }

    fn is_pushable(&self) -> bool {
        false
    }

    fn piston_push_reaction(&self) -> PushReaction {
        if self.is_marker() {
            PushReaction::Ignore
        } else {
            PushReaction::Normal
        }
    }

    fn get_pick_result(&self) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::ARMOR_STAND))
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_ARMOR_STAND_FALL,
            &sound_events::ENTITY_ARMOR_STAND_FALL,
        )
    }

    fn is_ignoring_block_triggers(&self) -> bool {
        self.is_marker()
    }

    fn is_marker_armor_stand(&self) -> bool {
        self.is_marker()
    }

    fn is_pickable(&self) -> bool {
        !self.is_marker()
    }

    fn kill(&self, _world: &World) {
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    fn skip_attack_interaction(&self, source: &dyn Entity) -> bool {
        if let Some(player) = source.as_player() {
            !self
                .level()
                .is_some_and(|l| l.may_interact(player, self.block_position()))
        } else {
            false
        }
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        let player_stack = player.inventory.lock().get_item_in_hand(hand).clone();
        if self.is_marker() || player_stack.item() == &*vanilla_items::NAME_TAG {
            return self.interact_entity(player, hand, location);
        }

        if player.is_spectator() {
            return InteractionResult::Success;
        }

        let item_in_hand_slot = self.get_equipment_slot_for_item(&player_stack);
        if player_stack.is_empty() {
            let clicked_slot = self.get_clicked_slot(location);
            let target_slot = if self.is_disabled(clicked_slot) {
                item_in_hand_slot
            } else {
                clicked_slot
            };
            if self.has_item_in_slot(target_slot)
                && self.swap_item(player, target_slot, &player_stack, hand)
            {
                return InteractionResult::SuccessServer;
            }
        } else {
            if self.is_disabled(item_in_hand_slot) {
                return InteractionResult::Fail;
            }

            if item_in_hand_slot.slot_type() == EquipmentSlotType::Hand && !self.show_arms() {
                return InteractionResult::Fail;
            }

            if self.swap_item(player, item_in_hand_slot, &player_stack, hand) {
                return InteractionResult::SuccessServer;
            }
        }

        self.interact_entity(player, hand, location)
    }

    fn is_invisible(&self) -> bool {
        *self.invisible.lock()
    }

    fn sound_source(&self) -> SoundSource {
        SoundSource::Neutral
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        let invisible = *self.invisible.lock();
        nbt.insert("Invisible", nbt_bool(invisible));
        nbt.insert("Small", nbt_bool(self.is_small()));
        nbt.insert("ShowArms", nbt_bool(self.show_arms()));
        nbt.insert("DisabledSlots", *self.disabled_slots.lock());
        nbt.insert("NoBasePlate", nbt_bool(!self.show_base_plate()));
        if self.is_marker() {
            nbt.insert("Marker", nbt_bool(true));
        }

        let mut pose_compound = NbtCompound::new();
        let head = self.head_pose();
        if head != DEFAULT_HEAD_POSE {
            pose_compound.insert("Head", save_rotations(head));
        }
        let body = self.body_pose();
        if body != DEFAULT_BODY_POSE {
            pose_compound.insert("Body", save_rotations(body));
        }
        let left_arm = self.left_arm_pose();
        if left_arm != DEFAULT_LEFT_ARM_POSE {
            pose_compound.insert("LeftArm", save_rotations(left_arm));
        }
        let right_arm = self.right_arm_pose();
        if right_arm != DEFAULT_RIGHT_ARM_POSE {
            pose_compound.insert("RightArm", save_rotations(right_arm));
        }
        let left_leg = self.left_leg_pose();
        if left_leg != DEFAULT_LEFT_LEG_POSE {
            pose_compound.insert("LeftLeg", save_rotations(left_leg));
        }
        let right_leg = self.right_leg_pose();
        if right_leg != DEFAULT_RIGHT_LEG_POSE {
            pose_compound.insert("RightLeg", save_rotations(right_leg));
        }
        if !pose_compound.is_empty() {
            nbt.insert("Pose", pose_compound);
        }

        let mut equipment_map = NbtCompound::new();
        let equipment = self.living_base.equipment().lock();
        for slot in EquipmentSlot::ALL {
            let item = equipment.get_ref(slot);
            if !item.is_empty() {
                equipment_map.insert(slot.name(), item.to_nbt_tag_ref());
            }
        }
        if !equipment_map.is_empty() {
            nbt.insert("equipment", equipment_map);
        }
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_invisible(nbt.byte("Invisible").is_some_and(|b| b != 0));
        self.set_small(nbt.byte("Small").is_some_and(|b| b != 0));
        self.set_show_arms(nbt.byte("ShowArms").is_some_and(|b| b != 0));
        *self.disabled_slots.lock() = nbt.int("DisabledSlots").unwrap_or(0);
        self.set_no_base_plate(nbt.byte("NoBasePlate").is_some_and(|b| b != 0));
        self.set_marker(nbt.byte("Marker").is_some_and(|b| b != 0));

        if let Some(pose_tag) = nbt.get("Pose")
            && let Some(pose_view) = pose_tag.compound()
        {
            if let Some(head) = load_rotations(pose_view.get("Head")) {
                self.set_head_pose(head);
            }
            if let Some(body) = load_rotations(pose_view.get("Body")) {
                self.set_body_pose(body);
            }
            if let Some(left_arm) = load_rotations(pose_view.get("LeftArm")) {
                self.set_left_arm_pose(left_arm);
            }
            if let Some(right_arm) = load_rotations(pose_view.get("RightArm")) {
                self.set_right_arm_pose(right_arm);
            }
            if let Some(left_leg) = load_rotations(pose_view.get("LeftLeg")) {
                self.set_left_leg_pose(left_leg);
            }
            if let Some(right_leg) = load_rotations(pose_view.get("RightLeg")) {
                self.set_right_leg_pose(right_leg);
            }
        }

        if let Some(equipment_tag) = nbt.get("equipment")
            && let Some(equipment_view) = equipment_tag.compound()
        {
            let mut equipment = self.living_base.equipment().lock();
            for slot in EquipmentSlot::ALL {
                if let Some(item_tag) = equipment_view.get(slot.name())
                    && let Some(item_view) = item_tag.compound()
                    && let Some(stack) = ItemStack::from_borrowed_compound(&item_view)
                {
                    equipment.set(slot, stack);
                }
            }
        }
    }
}

impl LivingEntity for ArmorStandEntity {
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
        Some(&sound_events::ENTITY_ARMOR_STAND_HIT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ARMOR_STAND_BREAK)
    }

    fn can_use_slot(&self, slot: EquipmentSlot) -> bool {
        slot != EquipmentSlot::Body && slot != EquipmentSlot::Saddle && !self.is_disabled(slot)
    }

    fn is_baby(&self) -> bool {
        self.is_small()
    }

    fn is_affected_by_potions(&self) -> bool {
        false
    }

    fn travel(&self, input: DVec3) -> Option<MoveResult> {
        if self.has_physics() {
            self.default_travel(input)
        } else {
            None
        }
    }

    fn set_y_body_rot(&self, y_body_rot: f32) {
        self.living_base().set_y_body_rot(y_body_rot);
        self.living_base().set_y_head_rot(y_body_rot);
    }

    fn set_y_head_rot(&self, y_head_rot: f32) {
        self.living_base().set_y_body_rot(y_head_rot);
        self.living_base().set_y_head_rot(y_head_rot);
    }

    fn push_entities(&self) {
        let Some(world) = self.level() else {
            return;
        };
        for entity in world.get_entities_in_aabb(&self.bounding_box()) {
            if entity.id() != self.id()
                && entity.entity_type() == &vanilla_entities::MINECART
                && self.position().distance_squared(entity.position()) <= 0.2
            {
                entity.push_entity(self);
            }
        }
    }

    fn hurt_server(&self, world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_removed() {
            return false;
        }

        let causing_entity = source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id));

        if !world.get_game_rule(&MOB_GRIEFING)
            && causing_entity
                .as_ref()
                .is_some_and(|e| e.as_mob().is_some())
        {
            return false;
        }

        if source.bypasses_invulnerability() {
            self.kill(world);
            return false;
        }

        if self.is_invulnerable_to(world, source) || self.is_invisible() || self.is_marker() {
            return false;
        }

        if source.is(&DamageTypeTag::IS_EXPLOSION) {
            self.broken_by_anything(source, world);
            self.kill(world);
            return false;
        }

        if source.is(&DamageTypeTag::IGNITES_ARMOR_STANDS) {
            if self.is_on_fire() {
                self.cause_damage(source, 0.15, world);
            } else {
                self.ignite_for_ticks(100);
            }
            return false;
        }

        if source.is(&DamageTypeTag::BURNS_ARMOR_STANDS) {
            if self.get_health() > 0.5 {
                self.cause_damage(source, 4.0, world);
            } else {
                self.broken_by_anything(source, world);
                self.kill(world);
            }
            return false;
        }

        let allow_incremental_breaking = source.is(&DamageTypeTag::CAN_BREAK_ARMOR_STAND);
        let should_kill = source.is(&DamageTypeTag::ALWAYS_KILLS_ARMOR_STANDS);

        if !allow_incremental_breaking && !should_kill {
            return false;
        }

        if let Some(player) = causing_entity.as_ref().and_then(|e| e.as_player())
            && !player.abilities.lock().may_build
        {
            return false;
        }

        if let Some(player) = causing_entity.as_ref().and_then(|e| e.as_player())
            && player.abilities.lock().instabuild
        {
            self.play_broken_sound();
            self.show_breaking_particles();
            self.kill(world);
            return true;
        }

        let time = world.game_time();
        let mut last_hit = self.last_hit.lock();
        let is_wobble = last_hit.is_none_or(|last| time - last > WOBBLE_TIME);
        if is_wobble && !should_kill {
            world.broadcast_to_entity_trackers(
                self.id(),
                CEntityEvent {
                    entity_id: self.id(),
                    event: EntityStatus::ArmorstandWobble,
                },
                None,
            );
            self.game_event(&vanilla_game_events::ENTITY_DAMAGE);
            *last_hit = Some(time);
        } else {
            self.broken_by_player(source, world);
            self.show_breaking_particles();
            self.kill(world);
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use simdnbt::borrow::read_compound;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_damage_types;

    use crate::test_support::test_world;

    #[test]
    fn armor_stand_default_dimensions_and_poses() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        assert_eq!(armor_stand.base().dimensions().width, 0.5);
        assert_eq!(armor_stand.base().dimensions().height, 1.975);
        assert_eq!(armor_stand.head_pose(), DEFAULT_HEAD_POSE);
        assert_eq!(armor_stand.body_pose(), DEFAULT_BODY_POSE);
        assert_eq!(armor_stand.left_arm_pose(), DEFAULT_LEFT_ARM_POSE);
        assert_eq!(armor_stand.right_arm_pose(), DEFAULT_RIGHT_ARM_POSE);
        assert_eq!(armor_stand.left_leg_pose(), DEFAULT_LEFT_LEG_POSE);
        assert_eq!(armor_stand.right_leg_pose(), DEFAULT_RIGHT_LEG_POSE);
        assert!(!armor_stand.is_small());
        assert!(!armor_stand.show_arms());
        assert!(armor_stand.show_base_plate());
        assert!(!armor_stand.is_marker());
    }

    #[test]
    fn armor_stand_small_and_marker_dimensions() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        armor_stand.set_small(true);
        assert!(armor_stand.is_small());
        assert_eq!(armor_stand.base().dimensions().width, 0.25);
        assert_eq!(armor_stand.base().dimensions().height, 0.9875);

        armor_stand.set_marker(true);
        assert!(armor_stand.is_marker());
        assert_eq!(armor_stand.base().dimensions().width, 0.0);
        assert_eq!(armor_stand.base().dimensions().height, 0.0);
        assert!(!armor_stand.has_physics());
    }

    #[test]
    fn armor_stand_client_flags_bitmask() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        armor_stand.set_small(true);
        armor_stand.set_show_arms(true);
        armor_stand.set_no_base_plate(true);
        armor_stand.set_marker(true);

        assert_eq!(
            armor_stand.client_flags(),
            CLIENT_FLAG_SMALL
                | CLIENT_FLAG_SHOW_ARMS
                | CLIENT_FLAG_NO_BASEPLATE
                | CLIENT_FLAG_MARKER
        );
        assert!(armor_stand.is_small());
        assert!(armor_stand.show_arms());
        assert!(!armor_stand.show_base_plate());
        assert!(armor_stand.is_marker());

        armor_stand.set_no_base_plate(false);
        assert!(armor_stand.show_base_plate());
    }

    #[test]
    fn armor_stand_poses_nbt_roundtrip() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        let custom_head = Rotations::new(10.0, 20.0, 30.0);
        let custom_arm = Rotations::new(-45.0, 15.0, 0.0);
        armor_stand.set_head_pose(custom_head);
        armor_stand.set_right_arm_pose(custom_arm);
        armor_stand.set_small(true);
        armor_stand.set_show_arms(true);
        armor_stand.set_disabled_slots(42);

        let mut bytes = Vec::new();
        let mut nbt = NbtCompound::new();
        armor_stand.save_additional(&mut nbt);
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));

        let loaded =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 2, DVec3::ZERO, Weak::new());
        loaded.load_additional((&borrowed).into());

        assert_eq!(loaded.head_pose(), custom_head);
        assert_eq!(loaded.right_arm_pose(), custom_arm);
        assert_eq!(loaded.body_pose(), DEFAULT_BODY_POSE);
        assert!(loaded.is_small());
        assert!(loaded.show_arms());
        assert_eq!(loaded.disabled_slots(), 42);
    }

    #[test]
    fn armor_stand_disabled_slots_rules() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        // Arms hidden by default -> hand slots disabled
        assert!(armor_stand.is_disabled(EquipmentSlot::MainHand));
        assert!(armor_stand.is_disabled(EquipmentSlot::OffHand));
        assert!(!armor_stand.is_disabled(EquipmentSlot::Head));
        assert!(!armor_stand.is_disabled(EquipmentSlot::Chest));

        armor_stand.set_show_arms(true);
        assert!(!armor_stand.is_disabled(EquipmentSlot::MainHand));
        assert!(!armor_stand.is_disabled(EquipmentSlot::OffHand));

        // Disable head slot via bitmask (head id = 4)
        armor_stand.set_disabled_slots(1 << EquipmentSlot::Head.id());
        assert!(armor_stand.is_disabled(EquipmentSlot::Head));
        assert!(!armor_stand.is_disabled(EquipmentSlot::Chest));
    }

    #[test]
    fn armor_stand_clicked_slot_detection() {
        init_vanilla_registry();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        // Equip all slots to test hit height detection
        armor_stand.living_base.equipment().lock().set(
            EquipmentSlot::Feet,
            ItemStack::new(&vanilla_items::IRON_BOOTS),
        );
        armor_stand.living_base.equipment().lock().set(
            EquipmentSlot::Legs,
            ItemStack::new(&vanilla_items::IRON_LEGGINGS),
        );
        armor_stand.living_base.equipment().lock().set(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::IRON_CHESTPLATE),
        );
        armor_stand.living_base.equipment().lock().set(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        );

        assert_eq!(
            armor_stand.get_clicked_slot(DVec3::new(0.0, 0.2, 0.0)),
            EquipmentSlot::Feet
        );
        assert_eq!(
            armor_stand.get_clicked_slot(DVec3::new(0.0, 0.6, 0.0)),
            EquipmentSlot::Legs
        );
        assert_eq!(
            armor_stand.get_clicked_slot(DVec3::new(0.0, 1.2, 0.0)),
            EquipmentSlot::Chest
        );
        assert_eq!(
            armor_stand.get_clicked_slot(DVec3::new(0.0, 1.7, 0.0)),
            EquipmentSlot::Head
        );
    }

    #[test]
    fn armor_stand_attackable_and_kill() {
        init_vanilla_registry();
        let world = test_world();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        assert!(armor_stand.attackable());
        assert!(!armor_stand.is_removed());

        armor_stand.kill(world);
        assert!(armor_stand.is_removed());
        assert_eq!(
            armor_stand.base().removal_reason(),
            Some(RemovalReason::Killed)
        );
    }

    #[test]
    fn armor_stand_lava_and_burn_damage() {
        init_vanilla_registry();
        let world = test_world();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        let lava_source = DamageSource::environment(&vanilla_damage_types::LAVA);
        // Lava is IS_FIRE but not CAN_BREAK_ARMOR_STAND, and not BURNS_ARMOR_STANDS (which is on_fire).
        // It does not break or damage armor stands incrementally unless on fire or burning.
        assert!(!armor_stand.hurt_server(world, &lava_source, 4.0));
        assert!(!armor_stand.is_removed());

        // On-fire source (BURNS_ARMOR_STANDS) damages armor stand
        let on_fire_source = DamageSource::environment(&vanilla_damage_types::ON_FIRE);
        assert!(!armor_stand.hurt_server(world, &on_fire_source, 4.0));
        assert!(armor_stand.get_health() < 20.0);

        // Subsequent burn kills armor stand when health drops <= 0.5
        armor_stand.set_health(0.4);
        armor_stand.set_remaining_fire_ticks(100);
        assert!(!armor_stand.hurt_server(world, &on_fire_source, 0.15));
        assert!(armor_stand.is_removed());
    }

    #[test]
    fn armor_stand_punch_wobble_and_break() {
        init_vanilla_registry();
        let world = test_world();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        let attack_source = DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK);

        // First punch -> wobbles (returns true)
        assert!(armor_stand.hurt_server(world, &attack_source, 1.0));
        assert!(!armor_stand.is_removed());

        // Second punch immediately within 5 ticks -> breaks and kills entity
        assert!(armor_stand.hurt_server(world, &attack_source, 1.0));
        assert!(armor_stand.is_removed());
    }

    #[test]
    fn armor_stand_get_pick_result_and_fire_ignition() {
        init_vanilla_registry();
        let world = test_world();
        let armor_stand =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new());

        let pick = armor_stand.get_pick_result();
        assert!(pick.is_some_and(|item| item.is(&vanilla_items::ARMOR_STAND)));

        let fire_source = DamageSource::environment(&vanilla_damage_types::IN_FIRE);
        armor_stand.hurt_server(world, &fire_source, 0.0);
        assert!(armor_stand.is_on_fire());
    }
}
