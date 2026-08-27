//! Vanilla armor-stand entity.
//!
//! Armor stands are living entities that are not mobs. They hold equipment,
//! expose pose/flag metadata, and use a dedicated interaction and damage path.

use std::sync::Weak;

use glam::DVec3;
use simdnbt::borrow::NbtCompound as BorrowedNbtCompoundView;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_macros::entity_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::data_components::vanilla_components::{CUSTOM_DATA, CUSTOM_NAME};
use steel_registry::enchantment_effect::EnchantmentEffectComponent;
use steel_registry::entity_data::Rotations;
use steel_registry::entity_type::{EntityDimensions, EntityTypeRef};
use steel_registry::item_stack::ItemStack;
use steel_registry::particle_type::{BlockParticleOption, ParticleData};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_entity_data::ArmorStandEntityData;
use steel_registry::vanilla_game_rules::MOB_GRIEFING;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_damage_type_tags, vanilla_entities, vanilla_game_events,
    vanilla_items, vanilla_particle_types,
};
use steel_utils::entity_events::EntityStatus;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::behavior::InteractionResult;
use crate::entity::damage::DamageSource;
use crate::entity::{
    Entity, EntityBase, EntityBaseLoad, EntityPose, EntitySyncedData, LivingEntity,
    LivingEntityBase, LivingEntitySyncedData, RemovalReason,
};
use crate::inventory::equipment::{EquipmentSlot, EquipmentSlotType};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

const CLIENT_FLAG_SMALL: i8 = 1;
const CLIENT_FLAG_SHOW_ARMS: i8 = 4;
const CLIENT_FLAG_NO_BASEPLATE: i8 = 8;
const CLIENT_FLAG_MARKER: i8 = 16;
const DISABLE_TAKING_OFFSET: i32 = 8;
const DISABLE_PUTTING_OFFSET: i32 = 16;
const WOBBLE_TIME: i64 = 5;
const IGNITE_TICKS: i32 = 100;
const FIRE_DAMAGE: f32 = 0.15;
const BURN_DAMAGE: f32 = 4.0;
const BROKEN_HEALTH: f32 = 0.5;
const MINECART_PUSH_DISTANCE_SQ: f64 = 0.2;
const BREAK_PARTICLE_COUNT: i32 = 10;
const BREAK_PARTICLE_SPEED: f64 = 0.05;
const BREAK_PARTICLE_Y_FACTOR: f64 = 2.0 / 3.0;
const BABY_EYE_HEIGHT: f32 = 0.9875;
const MARKER_DIMENSIONS: EntityDimensions = EntityDimensions::new(0.0, 0.0, 0.0);
const DEFAULT_HEAD_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
const DEFAULT_BODY_POSE: Rotations = Rotations::new(0.0, 0.0, 0.0);
const DEFAULT_LEFT_ARM_POSE: Rotations = Rotations::new(-10.0, 0.0, -10.0);
const DEFAULT_RIGHT_ARM_POSE: Rotations = Rotations::new(-15.0, 0.0, 10.0);
const DEFAULT_LEFT_LEG_POSE: Rotations = Rotations::new(-1.0, 0.0, -1.0);
const DEFAULT_RIGHT_LEG_POSE: Rotations = Rotations::new(1.0, 0.0, 1.0);

/// Vanilla armor-stand entity.
#[entity_behavior(class = "ArmorStand")]
pub struct ArmorStandEntity {
    base: EntityBase,
    entity_type: EntityTypeRef,
    living_base: LivingEntityBase,
    entity_data: SyncMutex<ArmorStandEntityData>,
    invisible: SyncMutex<bool>,
    last_hit: SyncMutex<i64>,
    disabled_slots: SyncMutex<i32>,
}

// SAFETY: This key is owned by Steel and uniquely identifies `ArmorStandEntity`.
unsafe impl DowncastType for ArmorStandEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:entity/armor_stand");
}

impl ArmorStandEntity {
    /// Creates a new armor stand.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef, id: i32, position: DVec3, world: Weak<World>) -> Self {
        Self::new_with_base(
            EntityBase::new(id, position, entity_type.dimensions, world),
            entity_type,
        )
    }

    /// Creates an armor stand from saved base data.
    #[must_use]
    pub fn from_saved(entity_type: EntityTypeRef, load: EntityBaseLoad) -> Self {
        Self::new_with_base(
            EntityBase::from_load(load, entity_type.dimensions),
            entity_type,
        )
    }

    fn new_with_base(base: EntityBase, entity_type: EntityTypeRef) -> Self {
        let living_base = LivingEntityBase::new(entity_type);
        let mut entity_data = ArmorStandEntityData::new();
        living_base.initialize_synced_data(&mut entity_data);
        Self {
            base,
            entity_type,
            living_base,
            entity_data: SyncMutex::new(entity_data),
            invisible: SyncMutex::new(false),
            last_hit: SyncMutex::new(0),
            disabled_slots: SyncMutex::new(0),
        }
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

        self.entity_data
            .set_base_invisible_flag(*self.invisible.lock());
        self.entity_data
            .set_base_glowing_flag(self.has_glowing_tag() || display.glowing);
    }

    fn client_flags(&self) -> i8 {
        *self.entity_data.lock().client_flags.get()
    }

    fn set_flag(&self, bit: i8, value: bool) {
        let mut entity_data = self.entity_data.lock();
        let flags = *entity_data.client_flags.get();
        let next = set_bit(flags, bit, value);
        if flags == next {
            return;
        }
        entity_data.client_flags.set(next);
        drop(entity_data);
        self.refresh_dimensions();
        self.sync_physics_state();
    }

    fn has_physics(&self) -> bool {
        !self.is_marker() && !self.is_no_gravity()
    }

    fn sync_physics_state(&self) {
        self.set_no_physics(!self.has_physics());
    }

    /// Returns vanilla `ArmorStand.isSmall`.
    #[must_use]
    pub fn is_small(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_SMALL) != 0
    }

    /// Sets vanilla `ArmorStand.setSmall`.
    pub fn set_small(&self, small: bool) {
        self.set_flag(CLIENT_FLAG_SMALL, small);
    }

    /// Returns vanilla `ArmorStand.showArms`.
    #[must_use]
    pub fn show_arms(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_SHOW_ARMS) != 0
    }

    /// Sets vanilla `ArmorStand.setShowArms`.
    pub fn set_show_arms(&self, show_arms: bool) {
        self.set_flag(CLIENT_FLAG_SHOW_ARMS, show_arms);
    }

    /// Returns vanilla `ArmorStand.showBasePlate`.
    #[must_use]
    pub fn show_base_plate(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_NO_BASEPLATE) == 0
    }

    /// Sets vanilla `ArmorStand.setNoBasePlate`.
    pub fn set_no_base_plate(&self, no_base_plate: bool) {
        self.set_flag(CLIENT_FLAG_NO_BASEPLATE, no_base_plate);
    }

    /// Returns vanilla `ArmorStand.isMarker`.
    #[must_use]
    pub fn is_marker(&self) -> bool {
        (self.client_flags() & CLIENT_FLAG_MARKER) != 0
    }

    /// Sets vanilla `ArmorStand.setMarker`.
    pub fn set_marker(&self, marker: bool) {
        self.set_flag(CLIENT_FLAG_MARKER, marker);
    }

    /// Returns vanilla `ArmorStand.isInvisible` NBT state.
    #[must_use]
    pub fn stand_invisible(&self) -> bool {
        *self.invisible.lock()
    }

    /// Sets vanilla `ArmorStand.setInvisible`.
    pub fn set_stand_invisible(&self, invisible: bool) {
        *self.invisible.lock() = invisible;
        self.entity_data.set_base_invisible_flag(invisible);
    }

    /// Returns vanilla `ArmorStand.disabledSlots`.
    #[must_use]
    pub fn disabled_slots(&self) -> i32 {
        *self.disabled_slots.lock()
    }

    /// Sets vanilla `ArmorStand.disabledSlots`.
    pub fn set_disabled_slots(&self, disabled_slots: i32) {
        *self.disabled_slots.lock() = disabled_slots;
    }

    fn is_disabled(&self, slot: EquipmentSlot) -> bool {
        let disabled = (self.disabled_slots() & (1 << slot.filter_bit(0))) != 0;
        disabled || (slot.slot_type() == EquipmentSlotType::Hand && !self.show_arms())
    }

    fn taking_disabled(&self, slot: EquipmentSlot) -> bool {
        (self.disabled_slots() & (1 << slot.filter_bit(DISABLE_TAKING_OFFSET))) != 0
    }

    fn putting_disabled(&self, slot: EquipmentSlot) -> bool {
        (self.disabled_slots() & (1 << slot.filter_bit(DISABLE_PUTTING_OFFSET))) != 0
    }

    fn set_slot(&self, slot: EquipmentSlot, stack: ItemStack) {
        self.living_base.equipment().lock().set(slot, stack);
    }

    fn read_pose_part(&self, read: impl FnOnce(&ArmorStandEntityData) -> Rotations) -> Rotations {
        read(&self.entity_data.lock())
    }

    fn write_pose_part(
        &self,
        write: impl FnOnce(&mut ArmorStandEntityData, Rotations),
        pose: Rotations,
    ) {
        write(&mut self.entity_data.lock(), pose);
    }

    /// Returns vanilla `ArmorStand.getHeadPose`.
    #[must_use]
    pub fn head_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.head_pose.get())
    }

    /// Sets vanilla `ArmorStand.setHeadPose`.
    pub fn set_head_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.head_pose.set(pose), pose);
    }

    /// Returns vanilla `ArmorStand.getBodyPose`.
    #[must_use]
    pub fn body_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.body_pose.get())
    }

    /// Sets vanilla `ArmorStand.setBodyPose`.
    pub fn set_body_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.body_pose.set(pose), pose);
    }

    /// Returns vanilla `ArmorStand.getLeftArmPose`.
    #[must_use]
    pub fn left_arm_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.left_arm_pose.get())
    }

    /// Sets vanilla `ArmorStand.setLeftArmPose`.
    pub fn set_left_arm_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.left_arm_pose.set(pose), pose);
    }

    /// Returns vanilla `ArmorStand.getRightArmPose`.
    #[must_use]
    pub fn right_arm_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.right_arm_pose.get())
    }

    /// Sets vanilla `ArmorStand.setRightArmPose`.
    pub fn set_right_arm_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.right_arm_pose.set(pose), pose);
    }

    /// Returns vanilla `ArmorStand.getLeftLegPose`.
    #[must_use]
    pub fn left_leg_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.left_leg_pose.get())
    }

    /// Sets vanilla `ArmorStand.setLeftLegPose`.
    pub fn set_left_leg_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.left_leg_pose.set(pose), pose);
    }

    /// Returns vanilla `ArmorStand.getRightLegPose`.
    #[must_use]
    pub fn right_leg_pose(&self) -> Rotations {
        self.read_pose_part(|data| *data.right_leg_pose.get())
    }

    /// Sets vanilla `ArmorStand.setRightLegPose`.
    pub fn set_right_leg_pose(&self, pose: Rotations) {
        self.write_pose_part(|data, pose| data.right_leg_pose.set(pose), pose);
    }

    fn clicked_slot(&self, location: DVec3) -> EquipmentSlot {
        let small = self.is_small();
        let scale = LivingEntity::get_scale(self) * LivingEntity::get_age_scale(self);
        let click_y = location.y / f64::from(scale);
        if (0.1..0.1 + if small { 0.8 } else { 0.45 }).contains(&click_y)
            && self.has_item_in_slot(EquipmentSlot::Feet)
        {
            EquipmentSlot::Feet
        } else if (0.9 + if small { 0.3 } else { 0.0 }..0.9 + if small { 1.0 } else { 0.7 })
            .contains(&click_y)
            && self.has_item_in_slot(EquipmentSlot::Chest)
        {
            EquipmentSlot::Chest
        } else if (0.4..0.4 + if small { 1.0 } else { 0.8 }).contains(&click_y)
            && self.has_item_in_slot(EquipmentSlot::Legs)
        {
            EquipmentSlot::Legs
        } else if click_y >= 1.6 && self.has_item_in_slot(EquipmentSlot::Head) {
            EquipmentSlot::Head
        } else if !self.has_item_in_slot(EquipmentSlot::MainHand)
            && self.has_item_in_slot(EquipmentSlot::OffHand)
        {
            EquipmentSlot::OffHand
        } else {
            EquipmentSlot::MainHand
        }
    }

    fn swap_item(&self, player: &Player, slot: EquipmentSlot, hand: InteractionHand) -> bool {
        let stand_stack = {
            let equipment = self.living_base.equipment().lock();
            equipment.get_ref(slot).clone()
        };
        if !stand_stack.is_empty() && self.taking_disabled(slot) {
            return false;
        }
        if stand_stack.is_empty() && self.putting_disabled(slot) {
            return false;
        }

        let mut inventory = player.inventory.lock();
        let player_empty = inventory.get_item_in_hand(hand).is_empty();
        let player_count = inventory.get_item_in_hand(hand).count;
        if player.has_infinite_materials() && stand_stack.is_empty() && !player_empty {
            let equipped = inventory.get_item_in_hand(hand).copy_with_count(1);
            drop(inventory);
            self.set_slot(slot, equipped);
            return true;
        }

        if player_empty || player_count <= 1 {
            let player_stack = inventory.get_item_in_hand(hand).clone();
            inventory.set_item_in_hand(hand, stand_stack);
            drop(inventory);
            self.set_slot(slot, player_stack);
            return true;
        }

        if !stand_stack.is_empty() {
            return false;
        }

        let taken = inventory.get_item_in_hand_mut(hand).split(1);
        drop(inventory);
        self.set_slot(slot, taken);
        true
    }

    fn baby_dimensions(&self) -> EntityDimensions {
        let scaled = self.entity_type.dimensions.scale(0.5);
        EntityDimensions {
            width: scaled.width,
            height: scaled.height,
            eye_height: BABY_EYE_HEIGHT,
            attachments: scaled.attachments,
        }
    }

    fn default_dimensions(&self) -> EntityDimensions {
        if self.is_marker() {
            MARKER_DIMENSIONS
        } else if self.is_small() {
            self.baby_dimensions()
        } else {
            self.entity_type.dimensions
        }
    }

    fn apply_locked_yaw(&self, yaw: f32) {
        self.base
            .set_old_rotation((yaw, self.base.old_rotation().1));
        self.living_base.set_y_body_rot_o(yaw);
        self.living_base.set_y_body_rot(yaw);
        self.living_base.set_y_head_rot_o(yaw);
        self.living_base.set_y_head_rot(yaw);
    }

    fn emit_damage_game_event(&self, source: &DamageSource) {
        let Some(world) = self.level() else {
            return;
        };
        let causing = source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id));
        world.game_event_at(
            &vanilla_game_events::ENTITY_DAMAGE,
            self.position(),
            &GameEventContext::new(causing.as_deref(), None),
        );
    }

    fn play_broken_sound(&self) {
        self.play_sound(&sound_events::ENTITY_ARMOR_STAND_BREAK, 1.0, 1.0);
    }

    fn show_breaking_particles(&self) {
        let Some(world) = self.level() else {
            return;
        };
        let dimensions = self.base.dimensions();
        let position = self.position();
        let width = f64::from(dimensions.width);
        let height = f64::from(dimensions.height);
        world.send_particles(
            ParticleData::new(
                &vanilla_particle_types::BLOCK,
                BlockParticleOption::new(vanilla_blocks::OAK_PLANKS.default_state()),
            ),
            DVec3::new(
                position.x,
                position.y + height * BREAK_PARTICLE_Y_FACTOR,
                position.z,
            ),
            BREAK_PARTICLE_COUNT,
            DVec3::new(width / 4.0, height / 4.0, width / 4.0),
            BREAK_PARTICLE_SPEED,
        );
    }

    fn cause_damage(&self, world: &World, source: &DamageSource, damage: f32) {
        let health = self.get_health() - damage;
        if health <= BROKEN_HEALTH {
            self.broken_by_anything(source);
            self.kill(world);
        } else {
            self.set_health(health);
            self.emit_damage_game_event(source);
        }
    }

    fn broken_by_player(&self, source: &DamageSource) {
        if let Some(world) = self.level() {
            let mut dropped = ItemStack::new(&vanilla_items::ARMOR_STAND);
            if let Some(custom_name) = self.custom_name() {
                dropped.set(CUSTOM_NAME, custom_name);
            }
            world.pop_resource(self.block_position(), dropped);
        }
        self.broken_by_anything(source);
    }

    fn broken_by_anything(&self, source: &DamageSource) {
        self.play_broken_sound();
        self.drop_all_death_loot(source);
        let Some(world) = self.level() else {
            return;
        };
        let drop_pos = self.block_position().above();
        for slot in EquipmentSlot::ALL {
            let item = self
                .living_base
                .equipment()
                .lock()
                .set(slot, ItemStack::empty());
            if item.is_empty()
                || item.has_enchantment_effect(EnchantmentEffectComponent::PreventEquipmentDrop)
            {
                continue;
            }
            world.pop_resource(drop_pos, item);
        }
    }

    fn save_pose(&self, nbt: &mut NbtCompound) {
        let mut pose = NbtCompound::new();
        insert_pose_if_non_default(&mut pose, "Head", self.head_pose(), DEFAULT_HEAD_POSE);
        insert_pose_if_non_default(&mut pose, "Body", self.body_pose(), DEFAULT_BODY_POSE);
        insert_pose_if_non_default(
            &mut pose,
            "LeftArm",
            self.left_arm_pose(),
            DEFAULT_LEFT_ARM_POSE,
        );
        insert_pose_if_non_default(
            &mut pose,
            "RightArm",
            self.right_arm_pose(),
            DEFAULT_RIGHT_ARM_POSE,
        );
        insert_pose_if_non_default(
            &mut pose,
            "LeftLeg",
            self.left_leg_pose(),
            DEFAULT_LEFT_LEG_POSE,
        );
        insert_pose_if_non_default(
            &mut pose,
            "RightLeg",
            self.right_leg_pose(),
            DEFAULT_RIGHT_LEG_POSE,
        );
        nbt.insert("Pose", NbtTag::Compound(pose));
    }

    fn load_pose(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        let Some(pose) = nbt.compound("Pose") else {
            return;
        };
        if let Some(rotations) = read_pose(&pose, "Head") {
            self.set_head_pose(rotations);
        }
        if let Some(rotations) = read_pose(&pose, "Body") {
            self.set_body_pose(rotations);
        }
        if let Some(rotations) = read_pose(&pose, "LeftArm") {
            self.set_left_arm_pose(rotations);
        }
        if let Some(rotations) = read_pose(&pose, "RightArm") {
            self.set_right_arm_pose(rotations);
        }
        if let Some(rotations) = read_pose(&pose, "LeftLeg") {
            self.set_left_leg_pose(rotations);
        }
        if let Some(rotations) = read_pose(&pose, "RightLeg") {
            self.set_right_leg_pose(rotations);
        }
    }

    /// Applies vanilla `Entity.applyComponentsFromItemStack` for spawn-from-item.
    pub fn apply_components_from_item_stack(&self, stack: &ItemStack) {
        if let Some(custom_name) = stack.get(CUSTOM_NAME) {
            self.set_custom_name(Some(custom_name.clone()));
        }
        if let Some(custom_data) = stack.get(CUSTOM_DATA) {
            self.set_custom_data(custom_data.copy_tag());
        }
    }
}

const fn set_bit(data: i8, bit: i8, value: bool) -> i8 {
    if value { data | bit } else { data & !bit }
}

fn insert_pose_if_non_default(
    pose: &mut NbtCompound,
    key: &str,
    rotations: Rotations,
    default: Rotations,
) {
    if rotations == default {
        return;
    }
    pose.insert(
        key,
        NbtList::Float(vec![rotations.x, rotations.y, rotations.z]),
    );
}

fn read_pose(pose: &BorrowedNbtCompoundView<'_, '_>, key: &str) -> Option<Rotations> {
    let values = pose.list(key)?.floats()?;
    if values.len() != 3 {
        return None;
    }
    Some(Rotations::new(values[0], values[1], values[2]))
}

impl Entity for ArmorStandEntity {
    fn base(&self) -> &EntityBase {
        &self.base
    }

    fn entity_type(&self) -> EntityTypeRef {
        self.entity_type
    }

    fn base_tick(&self) {
        LivingEntity::base_tick_living_entity(self);
    }

    fn dimensions_for_pose(&self, _pose: EntityPose) -> EntityDimensions {
        let dimensions = self.default_dimensions();
        if self.is_marker() {
            dimensions
        } else {
            dimensions.scale(LivingEntity::get_scale(self))
        }
    }

    fn refresh_dimensions(&self) {
        let position = self.position();
        let pose = Entity::pose(self);
        let new_dimensions = self.dimensions_for_pose(pose);
        self.base.set_pose_and_dimensions(pose, new_dimensions);
        if let Err(error) = self.base.try_set_position(position) {
            panic!(
                "failed to restore armor stand {} position after dimension refresh: {error}",
                self.base.id()
            );
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

    fn is_pushable(&self) -> bool {
        false
    }

    fn is_pickable(&self) -> bool {
        !self.is_removed() && !self.is_marker()
    }

    fn blocks_building(&self) -> bool {
        !self.is_marker()
    }

    fn is_marker_armor_stand(&self) -> bool {
        self.is_marker()
    }

    fn skip_attack_interaction(&self, source: &dyn Entity) -> bool {
        let Some(player) = source.as_player() else {
            return false;
        };
        let Some(world) = self.level() else {
            return false;
        };
        !world.may_interact(player, self.block_position())
    }

    fn is_effective_ai(&self) -> bool {
        self.is_server_driven_movement() && self.has_physics()
    }

    fn set_no_gravity(&self, no_gravity: bool) {
        self.base.set_no_gravity(no_gravity);
        if let Some(synced_data) = self.synced_data() {
            synced_data.set_no_gravity(no_gravity);
        }
        self.sync_physics_state();
    }

    fn fall_sounds(&self) -> (SoundEventRef, SoundEventRef) {
        (
            &sound_events::ENTITY_ARMOR_STAND_FALL,
            &sound_events::ENTITY_ARMOR_STAND_FALL,
        )
    }

    fn kill(&self, _world: &World) {
        self.set_removed(RemovalReason::Killed);
        self.game_event(&vanilla_game_events::ENTITY_DIE);
    }

    fn interact(
        &self,
        player: &Player,
        hand: InteractionHand,
        location: DVec3,
    ) -> InteractionResult {
        let item_stack = {
            let inventory = player.inventory.lock();
            inventory.get_item_in_hand(hand).clone()
        };
        if self.is_marker() || item_stack.is(&vanilla_items::NAME_TAG) {
            return InteractionResult::Pass;
        }
        if player.is_spectator() {
            return InteractionResult::Success;
        }

        let item_in_hand_slot = self.equipment_slot_for_item(&item_stack);
        if item_stack.is_empty() {
            let clicked_slot = self.clicked_slot(location);
            let target_slot = if self.is_disabled(clicked_slot) {
                item_in_hand_slot
            } else {
                clicked_slot
            };
            if self.has_item_in_slot(target_slot) && self.swap_item(player, target_slot, hand) {
                return InteractionResult::SuccessServer;
            }
        } else {
            if self.is_disabled(item_in_hand_slot) {
                return InteractionResult::Fail;
            }
            if item_in_hand_slot.slot_type() == EquipmentSlotType::Hand && !self.show_arms() {
                return InteractionResult::Fail;
            }
            if self.swap_item(player, item_in_hand_slot, hand) {
                return InteractionResult::SuccessServer;
            }
        }

        InteractionResult::Pass
    }

    fn save_additional(&self, nbt: &mut NbtCompound) {
        nbt.insert("Invisible", i8::from(self.stand_invisible()));
        nbt.insert("Small", i8::from(self.is_small()));
        nbt.insert("ShowArms", i8::from(self.show_arms()));
        nbt.insert("DisabledSlots", self.disabled_slots());
        nbt.insert("NoBasePlate", i8::from(!self.show_base_plate()));
        if self.is_marker() {
            nbt.insert("Marker", i8::from(true));
        }
        self.save_pose(nbt);
    }

    fn load_additional(&self, nbt: BorrowedNbtCompoundView<'_, '_>) {
        self.set_stand_invisible(nbt.byte("Invisible").is_some_and(|value| value != 0));
        self.set_small(nbt.byte("Small").is_some_and(|value| value != 0));
        self.set_show_arms(nbt.byte("ShowArms").is_some_and(|value| value != 0));
        self.set_disabled_slots(nbt.int("DisabledSlots").unwrap_or(0));
        self.set_no_base_plate(nbt.byte("NoBasePlate").is_some_and(|value| value != 0));
        self.set_marker(nbt.byte("Marker").is_some_and(|value| value != 0));
        self.sync_physics_state();
        self.load_pose(nbt);
    }
}

impl LivingEntity for ArmorStandEntity {
    fn living_base(&self) -> &LivingEntityBase {
        &self.living_base
    }

    fn living_synced_data(&self) -> Option<&dyn LivingEntitySyncedData> {
        Some(&self.entity_data)
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
        self.is_small()
    }

    fn can_use_slot(&self, slot: EquipmentSlot) -> bool {
        slot != EquipmentSlot::Body && slot != EquipmentSlot::Saddle && !self.is_disabled(slot)
    }

    fn is_affected_by_potions(&self) -> bool {
        false
    }

    fn is_living_attackable(&self) -> bool {
        false
    }

    fn can_be_seen_by_anyone(&self) -> bool {
        !self.is_invisible() && !self.is_marker()
    }

    fn hurt_sound(&self, _source: &DamageSource) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ARMOR_STAND_HIT)
    }

    fn death_sound(&self) -> Option<SoundEventRef> {
        Some(&sound_events::ENTITY_ARMOR_STAND_BREAK)
    }

    fn set_y_body_rot(&self, y_body_rot: f32) {
        self.apply_locked_yaw(y_body_rot);
    }

    fn set_y_head_rot(&self, y_head_rot: f32) {
        self.apply_locked_yaw(y_head_rot);
    }

    fn tick_head_turn(&self) {
        let yaw = self.rotation().0;
        let old_yaw = self.base.old_rotation().0;
        self.living_base.set_y_body_rot_o(old_yaw);
        self.living_base.set_y_body_rot(yaw);
    }

    fn travel(&self, input: DVec3) -> Option<MoveResult> {
        if self.has_physics() {
            self.default_travel(input)
        } else {
            None
        }
    }

    fn push_entities(&self) {
        let Some(world) = self.level() else {
            return;
        };
        if !world.tick_runs_normally() {
            return;
        }

        let position = self.position();
        for entity in world.get_entities_in_aabb_matching(&self.bounding_box(), |entity| {
            entity.entity_type() == &vanilla_entities::MINECART
        }) {
            if position.distance_squared(entity.position()) <= MINECART_PUSH_DISTANCE_SQ {
                entity.push_entity(self);
            }
        }
    }

    fn hurt_server(&self, world: &World, source: &DamageSource, _amount: f32) -> bool {
        if self.is_removed() {
            return false;
        }

        if !world.get_game_rule(&MOB_GRIEFING)
            && source
                .causing_entity_id
                .and_then(|id| world.get_entity_by_id(id))
                .is_some_and(|entity| entity.as_mob().is_some())
        {
            return false;
        }

        if source.bypasses_invulnerability() {
            self.kill(world);
            return false;
        }

        if self.is_invulnerable_to(world, source) || self.stand_invisible() || self.is_marker() {
            return false;
        }

        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IS_EXPLOSION) {
            self.broken_by_anything(source);
            self.kill(world);
            return false;
        }

        if source.is(&vanilla_damage_type_tags::DamageTypeTag::IGNITES_ARMOR_STANDS) {
            if self.is_on_fire() {
                self.cause_damage(world, source, FIRE_DAMAGE);
            } else {
                self.ignite_for_ticks(IGNITE_TICKS);
            }
            return false;
        }

        if source.is(&vanilla_damage_type_tags::DamageTypeTag::BURNS_ARMOR_STANDS)
            && self.get_health() > BROKEN_HEALTH
        {
            self.cause_damage(world, source, BURN_DAMAGE);
            return false;
        }

        let allow_incremental_breaking =
            source.is(&vanilla_damage_type_tags::DamageTypeTag::CAN_BREAK_ARMOR_STAND);
        let should_kill =
            source.is(&vanilla_damage_type_tags::DamageTypeTag::ALWAYS_KILLS_ARMOR_STANDS);
        if !allow_incremental_breaking && !should_kill {
            return false;
        }

        if source
            .causing_entity_id
            .and_then(|id| world.get_entity_by_id(id))
            .and_then(|entity| {
                entity
                    .as_player()
                    .map(|player| player.abilities.lock().may_build)
            })
            .is_some_and(|may_build| !may_build)
        {
            return false;
        }

        if self.source_is_creative_player(source) {
            self.play_broken_sound();
            self.show_breaking_particles();
            self.kill(world);
            return true;
        }

        let time = world.game_time();
        if time - *self.last_hit.lock() > WOBBLE_TIME && !should_kill {
            self.broadcast_entity_event(EntityStatus::ArmorstandWobble);
            self.emit_damage_game_event(source);
            *self.last_hit.lock() = time;
        } else {
            self.broken_by_player(source);
            self.show_breaking_particles();
            self.kill(world);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::{Arc, Weak};

    use simdnbt::borrow::read_compound;
    use steel_registry::{
        init_vanilla_registry, vanilla_attributes, vanilla_damage_types, vanilla_entities,
    };

    use crate::behavior::init_behaviors;
    use crate::entity::{Entity, SharedEntity};
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    fn stand() -> ArmorStandEntity {
        init_vanilla_registry();
        ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 1, DVec3::ZERO, Weak::new())
    }

    #[test]
    fn armor_stand_initializes_vanilla_living_attributes_and_health() {
        let stand = stand();
        assert_eq!(stand.get_health().to_bits(), 20.0_f32.to_bits());
        let attributes = stand.attributes().lock();
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::MAX_HEALTH)
                .to_bits(),
            20.0_f64.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::STEP_HEIGHT)
                .to_bits(),
            0.0_f64.to_bits()
        );
        assert_eq!(
            attributes
                .required_value(vanilla_attributes::MOVEMENT_SPEED)
                .to_bits(),
            0.7_f64.to_bits()
        );
    }

    #[test]
    fn try_as_dyn_exposes_armor_stand_living_entity_behavior() {
        let stand = stand();
        let entity = &stand as &dyn Entity;
        assert!(entity.is_living_entity());
        assert!(!entity.is_mob());
        let Some(living) = entity.as_living_entity() else {
            panic!("armor stand should expose living behavior");
        };
        assert_eq!(living.get_health().to_bits(), 20.0_f32.to_bits());
    }

    #[test]
    fn small_armor_stand_is_baby_and_uses_baby_dimensions() {
        let stand = stand();
        assert!(!LivingEntity::is_baby(&stand));
        stand.set_small(true);
        assert!(LivingEntity::is_baby(&stand));
        let dimensions = stand.dimensions_for_pose(Entity::pose(&stand));
        assert!((dimensions.width - 0.25).abs() < f32::EPSILON);
        assert!((dimensions.height - 0.9875).abs() < f32::EPSILON);
        assert!((dimensions.eye_height - BABY_EYE_HEIGHT).abs() < f32::EPSILON);
    }

    #[test]
    fn marker_armor_stand_uses_zero_dimensions_and_ignores_block_triggers() {
        let stand = stand();
        assert!(!stand.is_marker_armor_stand());
        assert!(stand.blocks_building());
        assert!(stand.is_pickable());
        stand.set_marker(true);
        assert!(stand.is_marker_armor_stand());
        assert!(!stand.blocks_building());
        assert!(!stand.is_pickable());
        assert!(!stand.is_pushable());
        assert!(stand.attackable());
        assert!(!LivingEntity::is_living_attackable(&stand));
        let dimensions = stand.dimensions_for_pose(Entity::pose(&stand));
        assert_eq!(dimensions.width.to_bits(), 0.0_f32.to_bits());
        assert_eq!(dimensions.height.to_bits(), 0.0_f32.to_bits());
        assert!(stand.no_physics());
    }

    #[test]
    fn armor_stand_cannot_use_body_or_saddle_slots() {
        let stand = stand();
        assert!(!stand.can_use_slot(EquipmentSlot::Body));
        assert!(!stand.can_use_slot(EquipmentSlot::Saddle));
        assert!(stand.can_use_slot(EquipmentSlot::Head));
        assert!(!stand.can_use_slot(EquipmentSlot::MainHand));
        stand.set_show_arms(true);
        assert!(stand.can_use_slot(EquipmentSlot::MainHand));
        assert!(stand.can_use_slot(EquipmentSlot::OffHand));
    }

    #[test]
    fn armor_stand_saves_and_loads_vanilla_additional_data() {
        let stand = stand();
        stand.set_stand_invisible(true);
        stand.set_small(true);
        stand.set_show_arms(true);
        stand.set_no_base_plate(true);
        stand.set_marker(true);
        stand.set_disabled_slots(0b1_0100);
        stand.set_head_pose(Rotations::new(12.0, 24.0, 36.0));
        stand.set_right_arm_pose(Rotations::new(1.0, 2.0, 3.0));

        let mut nbt = NbtCompound::new();
        stand.save_additional(&mut nbt);
        assert_eq!(nbt.byte("Invisible"), Some(1));
        assert_eq!(nbt.byte("Small"), Some(1));
        assert_eq!(nbt.byte("ShowArms"), Some(1));
        assert_eq!(nbt.byte("NoBasePlate"), Some(1));
        assert_eq!(nbt.byte("Marker"), Some(1));
        assert_eq!(nbt.int("DisabledSlots"), Some(0b1_0100));
        let Some(pose) = nbt.compound("Pose") else {
            panic!("pose should be saved");
        };
        assert_eq!(
            pose.get("Head"),
            Some(&NbtTag::List(NbtList::Float(vec![12.0, 24.0, 36.0])))
        );
        assert!(pose.get("Body").is_none());
        assert_eq!(
            pose.get("RightArm"),
            Some(&NbtTag::List(NbtList::Float(vec![1.0, 2.0, 3.0])))
        );

        let loaded =
            ArmorStandEntity::new(&vanilla_entities::ARMOR_STAND, 2, DVec3::ZERO, Weak::new());
        let mut bytes = Vec::new();
        nbt.write(&mut bytes);
        let borrowed = read_compound(&mut Cursor::new(&bytes))
            .unwrap_or_else(|error| panic!("reborrow failed: {error}"));
        loaded.load_additional((&borrowed).into());

        assert!(loaded.stand_invisible());
        assert!(loaded.is_small());
        assert!(loaded.show_arms());
        assert!(!loaded.show_base_plate());
        assert!(loaded.is_marker());
        assert_eq!(loaded.disabled_slots(), 0b1_0100);
        assert_eq!(loaded.head_pose(), Rotations::new(12.0, 24.0, 36.0));
        assert_eq!(loaded.right_arm_pose(), Rotations::new(1.0, 2.0, 3.0));
        assert_eq!(loaded.body_pose(), DEFAULT_BODY_POSE);
        assert!(loaded.no_physics());
    }

    #[test]
    fn clicked_slot_uses_vanilla_height_bands() {
        let stand = stand();
        stand.set_slot(
            EquipmentSlot::Feet,
            ItemStack::new(&vanilla_items::IRON_BOOTS),
        );
        stand.set_slot(
            EquipmentSlot::Legs,
            ItemStack::new(&vanilla_items::IRON_LEGGINGS),
        );
        stand.set_slot(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::IRON_CHESTPLATE),
        );
        stand.set_slot(
            EquipmentSlot::Head,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        );
        assert_eq!(
            stand.clicked_slot(DVec3::new(0.0, 0.2, 0.0)),
            EquipmentSlot::Feet
        );
        // Adult feet occupies [0.1, 0.55); legs still match [0.55, 0.9).
        assert_eq!(
            stand.clicked_slot(DVec3::new(0.0, 0.6, 0.0)),
            EquipmentSlot::Legs
        );
        assert_eq!(
            stand.clicked_slot(DVec3::new(0.0, 1.1, 0.0)),
            EquipmentSlot::Chest
        );
        assert_eq!(
            stand.clicked_slot(DVec3::new(0.0, 1.7, 0.0)),
            EquipmentSlot::Head
        );
    }

    #[test]
    fn interact_equips_helmet_and_rejects_hand_items_without_arms() {
        init_vanilla_registry();
        let world = fresh_test_world("armor_stand_swap");
        let stand = ArmorStandEntity::new(
            &vanilla_entities::ARMOR_STAND,
            1,
            DVec3::ZERO,
            Arc::downgrade(&world),
        );
        let player = TestPlayerBuilder::new(world, "Swapper", 2).build();
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::STICK),
        );
        assert_eq!(
            stand.interact(&player, InteractionHand::MainHand, DVec3::ZERO),
            InteractionResult::Fail
        );

        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::new(&vanilla_items::IRON_HELMET),
        );
        assert_eq!(
            stand.interact(&player, InteractionHand::MainHand, DVec3::ZERO),
            InteractionResult::SuccessServer
        );
        assert!(stand.has_item_in_slot(EquipmentSlot::Head));
        assert!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .is_empty()
        );
    }

    #[test]
    fn player_punch_reaches_armor_stand_hurt() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("armor_stand_attack");
        let stand: SharedEntity = Arc::new(ArmorStandEntity::new(
            &vanilla_entities::ARMOR_STAND,
            1,
            DVec3::ZERO,
            Arc::downgrade(&world),
        ));
        let player = TestPlayerBuilder::new(world, "Attacker", 2).build();

        assert!(
            player.attack(&stand),
            "vanilla Entity.isAttackable is true, so player punches must reach ArmorStand.hurtServer"
        );
    }

    fn player_attack_source() -> DamageSource {
        DamageSource::environment(&vanilla_damage_types::PLAYER_ATTACK)
    }

    #[test]
    fn fist_wobbles_then_breaks_within_five_ticks() {
        init_vanilla_registry();
        let world = fresh_test_world("armor_stand_two_hit");
        world.level_data.write().set_game_time(100);
        let stand = ArmorStandEntity::new(
            &vanilla_entities::ARMOR_STAND,
            1,
            DVec3::ZERO,
            Arc::downgrade(&world),
        );
        let source = player_attack_source();

        assert!(stand.hurt(&world, &source, 1.0));
        assert!(!stand.is_removed());

        world.level_data.write().set_game_time(105);
        assert!(stand.hurt(&world, &source, 1.0));
        assert!(stand.is_removed());
    }

    #[test]
    fn fist_second_hit_after_wobble_window_does_not_break() {
        init_vanilla_registry();
        let world = fresh_test_world("armor_stand_slow_hit");
        world.level_data.write().set_game_time(100);
        let stand = ArmorStandEntity::new(
            &vanilla_entities::ARMOR_STAND,
            1,
            DVec3::ZERO,
            Arc::downgrade(&world),
        );
        let source = player_attack_source();

        assert!(stand.hurt(&world, &source, 1.0));
        assert!(!stand.is_removed());

        world.level_data.write().set_game_time(106);
        assert!(stand.hurt(&world, &source, 1.0));
        assert!(!stand.is_removed());
    }
}
