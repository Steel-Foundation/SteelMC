//! Shared fields for all living entities.
//!
//! Mirrors the runtime fields that vanilla defines on `LivingEntity` (and
//! `Entity` for `invulnerableTime`). Entities that implement `LivingEntity`
//! embed this struct and expose it via `LivingEntity::living_base()`, just like
//! `EntityBase` is used for core `Entity` fields.

use std::{array, sync::Arc};

use glam::DVec3;
use rustc_hash::FxHashMap;
use steel_protocol::packets::game::{CRemoveMobEffect, CUpdateMobEffect, MobEffectPacketFlags};
use steel_registry::RegistryEntry;
use steel_registry::attribute::AttributeRef;
use steel_registry::entity_data::{ParticleData, ParticleList, ParticleOptions};
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_entity_data::VanillaLivingEntityData;
use steel_registry::vanilla_mob_effects;
use steel_utils::locks::SyncMutex;
use steel_utils::types::InteractionHand;
use steel_utils::{BlockPos, Identifier};
use uuid::Uuid;

use crate::entity::attribute::{AttributeMap, AttributeModifier, AttributeModifierOperation};
use crate::entity::damage::DamageSource;
use crate::entity::{LivingEntity, SharedEntity, WeakEntity};
use crate::inventory::equipment::{EntityEquipment, EquipmentSlot};

/// Duration in ticks of the death animation before entity removal.
pub const DEATH_DURATION: i32 = 20;
/// Vanilla default `SwingAnimation` duration in ticks.
pub const DEFAULT_SWING_DURATION: i32 = 6;
const INFINITE_EFFECT_DURATION: i32 = -1;
const MIN_EFFECT_AMPLIFIER: i32 = 0;
const MAX_EFFECT_AMPLIFIER: i32 = 255;
const AMBIENT_EFFECT_ALPHA: i32 = 38;
const VISIBLE_EFFECT_ALPHA: i32 = 255;
const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;
const POST_IMPULSE_GRACE_TICKS: i32 = 40;

/// Runtime mob-effect state.
///
/// Mirrors vanilla `MobEffectInstance` state that affects server-side living
/// physics and client synchronization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobEffectInstance {
    effect: MobEffectRef,
    duration: i32,
    amplifier: i32,
    ambient: bool,
    visible: bool,
    show_icon: bool,
    hidden_effect: Option<Box<MobEffectInstance>>,
}

/// Active mob-effect state stored on a living entity.
pub type ActiveMobEffect = MobEffectInstance;

impl MobEffectInstance {
    /// Creates infinite active mob-effect state for internal physics tests and hooks.
    #[must_use]
    pub const fn new(effect: MobEffectRef, amplifier: i32) -> Self {
        Self::with_duration(effect, INFINITE_EFFECT_DURATION, amplifier)
    }

    /// Creates active mob-effect state with vanilla default visibility flags.
    #[must_use]
    pub const fn with_duration(effect: MobEffectRef, duration: i32, amplifier: i32) -> Self {
        Self {
            effect,
            duration,
            amplifier: clamp_effect_amplifier(amplifier),
            ambient: false,
            visible: true,
            show_icon: true,
            hidden_effect: None,
        }
    }

    /// Sets whether this effect is ambient.
    #[must_use]
    pub const fn with_ambient(mut self, ambient: bool) -> Self {
        self.ambient = ambient;
        self
    }

    /// Sets whether this effect should show particles.
    #[must_use]
    pub const fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Sets whether this effect should show its inventory icon.
    #[must_use]
    pub const fn with_show_icon(mut self, show_icon: bool) -> Self {
        self.show_icon = show_icon;
        self
    }

    /// Returns the mob effect.
    #[must_use]
    pub const fn effect(&self) -> MobEffectRef {
        self.effect
    }

    /// Returns vanilla `MobEffectInstance.getDuration()`.
    #[must_use]
    pub const fn duration(&self) -> i32 {
        self.duration
    }

    /// Returns vanilla `MobEffectInstance.getAmplifier()`.
    #[must_use]
    pub const fn amplifier(&self) -> i32 {
        self.amplifier
    }

    /// Returns vanilla `MobEffectInstance.isAmbient()`.
    #[must_use]
    pub const fn is_ambient(&self) -> bool {
        self.ambient
    }

    /// Returns vanilla `MobEffectInstance.isVisible()`.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
    }

    /// Returns vanilla `MobEffectInstance.showIcon()`.
    #[must_use]
    pub const fn show_icon(&self) -> bool {
        self.show_icon
    }

    /// Returns whether this effect uses vanilla's infinite-duration sentinel.
    #[must_use]
    pub const fn is_infinite_duration(&self) -> bool {
        self.duration == INFINITE_EFFECT_DURATION
    }

    #[must_use]
    const fn has_remaining_duration(&self) -> bool {
        self.is_infinite_duration() || self.duration > 0
    }

    #[must_use]
    const fn is_shorter_duration_than(&self, other: &Self) -> bool {
        !self.is_infinite_duration()
            && (self.duration < other.duration || other.is_infinite_duration())
    }

    /// Merges another instance of the same effect into this instance.
    ///
    /// Mirrors vanilla `MobEffectInstance.update`.
    pub fn update(&mut self, take_over: Self) -> bool {
        let mut changed = false;
        let take_over_ambient = take_over.ambient;
        let take_over_visible = take_over.visible;
        let take_over_show_icon = take_over.show_icon;
        if take_over.amplifier > self.amplifier {
            if take_over.is_shorter_duration_than(self) {
                let previous_hidden_effect = self.hidden_effect.take();
                let mut hidden = self.clone();
                hidden.hidden_effect = previous_hidden_effect;
                self.hidden_effect = Some(Box::new(hidden));
            }

            self.amplifier = take_over.amplifier;
            self.duration = take_over.duration;
            changed = true;
        } else if self.is_shorter_duration_than(&take_over) {
            if take_over.amplifier == self.amplifier {
                self.duration = take_over.duration;
                changed = true;
            } else if let Some(hidden_effect) = &mut self.hidden_effect {
                hidden_effect.update(take_over);
            } else {
                self.hidden_effect = Some(Box::new(take_over));
            }
        }

        if (!take_over_ambient && self.ambient) || changed {
            self.ambient = take_over_ambient;
            changed = true;
        }

        if take_over_visible != self.visible {
            self.visible = take_over_visible;
            changed = true;
        }

        if take_over_show_icon != self.show_icon {
            self.show_icon = take_over_show_icon;
            changed = true;
        }

        changed
    }

    fn tick_duration(&mut self) -> MobEffectTickResult {
        if !self.has_remaining_duration() {
            return MobEffectTickResult::Expired;
        }

        // TODO: Run effect-specific server ticks such as poison, wither, regeneration,
        // hunger, saturation, and bad omen once those damage/food/raid hooks exist.
        self.tick_down_duration();
        if self.downgrade_to_hidden_effect() {
            return MobEffectTickResult::Active { downgraded: true };
        }
        if self.has_remaining_duration() {
            MobEffectTickResult::Active { downgraded: false }
        } else {
            MobEffectTickResult::Expired
        }
    }

    fn tick_down_duration(&mut self) {
        if let Some(hidden_effect) = &mut self.hidden_effect {
            hidden_effect.tick_down_duration();
        }

        if !self.is_infinite_duration() && self.duration != 0 {
            self.duration -= 1;
        }
    }

    fn downgrade_to_hidden_effect(&mut self) -> bool {
        if self.duration != 0 {
            return false;
        }

        let Some(hidden_effect) = self.hidden_effect.take() else {
            return false;
        };
        let MobEffectInstance {
            duration,
            amplifier,
            ambient,
            visible,
            show_icon,
            hidden_effect,
            ..
        } = *hidden_effect;
        self.duration = duration;
        self.amplifier = amplifier;
        self.ambient = ambient;
        self.visible = visible;
        self.show_icon = show_icon;
        self.hidden_effect = hidden_effect;
        true
    }

    const fn particle_color(&self) -> i32 {
        let alpha = if self.ambient {
            AMBIENT_EFFECT_ALPHA
        } else {
            VISIBLE_EFFECT_ALPHA
        };
        let color = ((alpha << 24) | (self.effect.color & 0x00ff_ffff)) as u32;
        color as i32
    }
}

const fn clamp_effect_amplifier(amplifier: i32) -> i32 {
    if amplifier < MIN_EFFECT_AMPLIFIER {
        MIN_EFFECT_AMPLIFIER
    } else if amplifier > MAX_EFFECT_AMPLIFIER {
        MAX_EFFECT_AMPLIFIER
    } else {
        amplifier
    }
}

enum MobEffectTickResult {
    Active { downgraded: bool },
    Expired,
}

/// A queued mob-effect packet change produced by living effect state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobEffectSyncChange {
    /// Add or update a mob effect.
    Update {
        /// The active effect instance to encode.
        effect: MobEffectInstance,
        /// Whether the owner-client packet should use vanilla's blend flag.
        blend_for_self: bool,
    },
    /// Remove a mob effect.
    Remove {
        /// The effect type to remove.
        effect: MobEffectRef,
    },
}

impl MobEffectSyncChange {
    /// Builds the clientbound packet for a concrete recipient.
    #[must_use]
    pub fn packet(&self, entity_id: i32, is_self_recipient: bool) -> MobEffectSyncPacket {
        match self {
            Self::Update {
                effect,
                blend_for_self,
            } => MobEffectSyncPacket::Update(CUpdateMobEffect::new(
                entity_id,
                effect.effect,
                effect.amplifier,
                effect.duration,
                MobEffectPacketFlags {
                    ambient: effect.ambient,
                    visible: effect.visible,
                    show_icon: effect.show_icon,
                    blend: *blend_for_self && is_self_recipient,
                },
            )),
            Self::Remove { effect } => {
                MobEffectSyncPacket::Remove(CRemoveMobEffect::new(entity_id, effect))
            }
        }
    }
}

/// Concrete mob-effect packet ready to send to a player connection.
#[derive(Debug, Clone)]
pub enum MobEffectSyncPacket {
    /// Add/update mob-effect packet.
    Update(CUpdateMobEffect),
    /// Remove mob-effect packet.
    Remove(CRemoveMobEffect),
}

/// Synchronized living entity-data values derived from active mob effects.
#[derive(Debug, Clone, PartialEq)]
pub struct MobEffectDisplayState {
    /// Visible effect particles for `LivingEntity.DATA_EFFECT_PARTICLES`.
    pub particles: ParticleList,
    /// Whether all active effects are ambient.
    pub ambient: bool,
    /// Whether the shared invisible flag should be set by active effects.
    pub invisible: bool,
    /// Whether the shared glowing flag should be set by active effects.
    pub glowing: bool,
}

/// Movement input stored on vanilla `LivingEntity`.
///
/// Vanilla names these fields `xxa`, `yya`, and `zza`; Steel uses axis names
/// so AI/pathfinding code can set intent without carrying obfuscated names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivingTravelInput {
    sideways: f32,
    vertical: f32,
    forward: f32,
}

impl LivingTravelInput {
    /// No travel input.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    /// Creates living travel input.
    #[must_use]
    pub const fn new(sideways: f32, vertical: f32, forward: f32) -> Self {
        Self {
            sideways,
            vertical,
            forward,
        }
    }

    /// Returns sideways movement input.
    #[must_use]
    pub const fn sideways(self) -> f32 {
        self.sideways
    }

    /// Returns vertical movement input.
    #[must_use]
    pub const fn vertical(self) -> f32 {
        self.vertical
    }

    /// Returns forward movement input.
    #[must_use]
    pub const fn forward(self) -> f32 {
        self.forward
    }

    /// Returns input after vanilla `LivingEntity.applyInput()` damping.
    #[must_use]
    pub const fn dampened(self) -> Self {
        Self {
            sideways: self.sideways * 0.98,
            vertical: self.vertical,
            forward: self.forward * 0.98,
        }
    }
}

/// Vanilla living-entity body/head rotation state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivingRotationState {
    y_body_rot: f32,
    y_body_rot_o: f32,
    y_head_rot: f32,
    y_head_rot_o: f32,
}

impl LivingRotationState {
    /// Creates default living rotation state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            y_body_rot: 0.0,
            y_body_rot_o: 0.0,
            y_head_rot: 0.0,
            y_head_rot_o: 0.0,
        }
    }

    /// Returns vanilla `yBodyRot`.
    #[must_use]
    pub const fn y_body_rot(self) -> f32 {
        self.y_body_rot
    }

    /// Returns vanilla `yBodyRotO`.
    #[must_use]
    pub const fn y_body_rot_o(self) -> f32 {
        self.y_body_rot_o
    }

    /// Returns vanilla `yHeadRot`.
    #[must_use]
    pub const fn y_head_rot(self) -> f32 {
        self.y_head_rot
    }

    /// Returns vanilla `yHeadRotO`.
    #[must_use]
    pub const fn y_head_rot_o(self) -> f32 {
        self.y_head_rot_o
    }
}

impl Default for LivingRotationState {
    fn default() -> Self {
        Self::new()
    }
}

/// Vanilla arm-swing animation state stored on `LivingEntity`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LivingSwingState {
    swinging: bool,
    swinging_arm: Option<InteractionHand>,
    swing_time: i32,
    old_attack_anim: f32,
    attack_anim: f32,
}

impl LivingSwingState {
    /// Creates empty vanilla swing animation state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            swinging: false,
            swinging_arm: None,
            swing_time: 0,
            old_attack_anim: 0.0,
            attack_anim: 0.0,
        }
    }

    /// Returns vanilla `LivingEntity.swinging`.
    #[must_use]
    pub const fn swinging(self) -> bool {
        self.swinging
    }

    /// Returns vanilla `LivingEntity.swingingArm`.
    #[must_use]
    pub const fn swinging_arm(self) -> Option<InteractionHand> {
        self.swinging_arm
    }

    /// Returns vanilla `LivingEntity.swingTime`.
    #[must_use]
    pub const fn swing_time(self) -> i32 {
        self.swing_time
    }

    /// Returns vanilla `LivingEntity.oAttackAnim`.
    #[must_use]
    pub const fn old_attack_anim(self) -> f32 {
        self.old_attack_anim
    }

    /// Returns vanilla `LivingEntity.attackAnim`.
    #[must_use]
    pub const fn attack_anim(self) -> f32 {
        self.attack_anim
    }
}

impl Default for LivingSwingState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct LivingEntityState {
    effects_dirty: bool,
    death_processed: bool,
    invulnerable_time: i32,
    last_hurt: f32,
    last_hurt_by_player: Option<Uuid>,
    last_hurt_by_player_memory_time: i32,
    last_hurt_by_mob: Option<WeakEntity>,
    last_hurt_by_mob_timestamp: i32,
    last_hurt_mob: Option<WeakEntity>,
    last_hurt_mob_timestamp: i32,
    last_damage_source: Option<DamageSource>,
    last_damage_stamp: i64,
    absorption_amount: f32,
    skip_drop_experience: bool,
    death_time: i32,
    speed: f32,
    current_impulse_impact_pos: Option<DVec3>,
    current_impulse_context_reset_grace_time: i32,
    fall_flying: bool,
    fall_flying_ticks: i32,
    sprinting: bool,
    sleeping_pos: Option<BlockPos>,
    last_climbable_pos: Option<BlockPos>,
    discard_friction: bool,
    jumping: bool,
    travel_input: LivingTravelInput,
    rotation: LivingRotationState,
    swing: LivingSwingState,
    no_jump_delay: i32,
    no_action_time: i32,
}

impl LivingEntityState {
    const fn new(speed: f32) -> Self {
        Self {
            effects_dirty: false,
            death_processed: false,
            invulnerable_time: 0,
            last_hurt: 0.0,
            last_hurt_by_player: None,
            last_hurt_by_player_memory_time: 0,
            last_hurt_by_mob: None,
            last_hurt_by_mob_timestamp: 0,
            last_hurt_mob: None,
            last_hurt_mob_timestamp: 0,
            last_damage_source: None,
            last_damage_stamp: 0,
            absorption_amount: 0.0,
            skip_drop_experience: false,
            death_time: 0,
            speed,
            current_impulse_impact_pos: None,
            current_impulse_context_reset_grace_time: 0,
            fall_flying: false,
            fall_flying_ticks: 0,
            sprinting: false,
            sleeping_pos: None,
            last_climbable_pos: None,
            discard_friction: false,
            jumping: false,
            travel_input: LivingTravelInput::ZERO,
            rotation: LivingRotationState::new(),
            swing: LivingSwingState::new(),
            no_jump_delay: 0,
            no_action_time: 0,
        }
    }

    const fn reset_death_state(&mut self) {
        self.death_processed = false;
        self.death_time = 0;
        self.invulnerable_time = 0;
        self.last_hurt = 0.0;
        self.absorption_amount = 0.0;
        self.skip_drop_experience = false;
    }
}

/// Common runtime fields shared by all living entities.
///
/// **Deviation from vanilla:** Vanilla calls this guard `LivingEntity.dead`,
/// but it means death side effects have been processed, not health is zero.
/// `ServerPlayer.die()` does NOT call `super.die()` and never sets that field.
/// Steel uses this guard for players too because it reuses the same `Player`
/// instance; health remains the source of truth for dead-or-dying checks such
/// as client respawn requests.
pub struct LivingEntityBase {
    state: SyncMutex<LivingEntityState>,
    attributes: SyncMutex<AttributeMap>,
    active_mob_effects: SyncMutex<FxHashMap<MobEffectRef, ActiveMobEffect>>,
    dirty_mob_effects: SyncMutex<Vec<MobEffectSyncChange>>,
    equipment: SyncMutex<EntityEquipment>,
    equipment_attribute_modifiers:
        SyncMutex<[Vec<EquipmentAttributeModifierKey>; EquipmentSlot::ALL.len()]>,
}

#[derive(Debug)]
struct EquipmentAttributeModifierKey {
    attribute: AttributeRef,
    id: Identifier,
}

impl LivingEntityBase {
    /// Creates living runtime state from an entity type's default attributes.
    #[must_use]
    pub fn new(entity_type: EntityTypeRef) -> Self {
        Self::with_attributes(AttributeMap::new_for_entity(entity_type))
    }

    /// Creates living runtime state from an explicit attribute map.
    #[must_use]
    pub fn with_attributes(attributes: AttributeMap) -> Self {
        let speed = attributes.required_value(vanilla_attributes::MOVEMENT_SPEED) as f32;

        Self {
            state: SyncMutex::new(LivingEntityState::new(speed)),
            attributes: SyncMutex::new(attributes),
            active_mob_effects: SyncMutex::new(FxHashMap::default()),
            dirty_mob_effects: SyncMutex::new(Vec::new()),
            equipment: SyncMutex::new(EntityEquipment::new()),
            equipment_attribute_modifiers: SyncMutex::new(array::from_fn(|_| Vec::new())),
        }
    }

    /// Returns this entity's attribute map.
    #[inline]
    pub const fn attributes(&self) -> &SyncMutex<AttributeMap> {
        &self.attributes
    }

    /// Applies vanilla constructor-time synced-data mutations for living entities.
    ///
    /// Vanilla defines `DATA_HEALTH_ID` as `1.0F`, then `LivingEntity` constructs
    /// its attribute map and calls `setHealth(getMaxHealth())`.
    pub fn initialize_synced_data<T: VanillaLivingEntityData>(&self, entity_data: &mut T) {
        let max_health = self
            .attributes
            .lock()
            .required_value(vanilla_attributes::MAX_HEALTH) as f32;
        entity_data.living_entity_mut().health.set(max_health);
    }

    /// Returns vanilla `LivingEntity.equipment` storage.
    #[inline]
    pub const fn equipment(&self) -> &SyncMutex<EntityEquipment> {
        &self.equipment
    }

    /// Returns vanilla living body/head rotation state.
    #[must_use]
    pub fn rotation_state(&self) -> LivingRotationState {
        self.state.lock().rotation
    }

    /// Returns vanilla arm-swing animation state.
    #[must_use]
    pub fn swing_state(&self) -> LivingSwingState {
        self.state.lock().swing
    }

    /// Returns vanilla `yBodyRot`.
    #[must_use]
    pub fn y_body_rot(&self) -> f32 {
        self.state.lock().rotation.y_body_rot
    }

    /// Sets vanilla `yBodyRot`.
    pub fn set_y_body_rot(&self, y_body_rot: f32) {
        self.state.lock().rotation.y_body_rot = y_body_rot;
    }

    /// Returns vanilla `yHeadRot`.
    #[must_use]
    pub fn y_head_rot(&self) -> f32 {
        self.state.lock().rotation.y_head_rot
    }

    /// Sets vanilla `yHeadRot`.
    pub fn set_y_head_rot(&self, y_head_rot: f32) {
        self.state.lock().rotation.y_head_rot = y_head_rot;
    }

    /// Copies current living head/body rotations to their old-rotation fields.
    pub fn advance_rotation_for_base_tick(&self) {
        let mut state = self.state.lock();
        state.rotation.y_head_rot_o = state.rotation.y_head_rot;
        state.rotation.y_body_rot_o = state.rotation.y_body_rot;
    }

    /// Copies current attack animation to vanilla `oAttackAnim`.
    pub fn advance_attack_animation_for_base_tick(&self) {
        let mut state = self.state.lock();
        state.swing.old_attack_anim = state.swing.attack_anim;
    }

    /// Starts vanilla `LivingEntity.swing` state if the swing gate allows it.
    pub fn start_swing(&self, hand: InteractionHand, current_swing_duration: i32) -> bool {
        let mut state = self.state.lock();
        let swing = &mut state.swing;
        if swing.swinging && swing.swing_time < current_swing_duration / 2 && swing.swing_time >= 0
        {
            return false;
        }

        swing.swing_time = -1;
        swing.swinging = true;
        swing.swinging_arm = Some(hand);
        true
    }

    /// Updates vanilla `LivingEntity.swingTime` and `attackAnim`.
    pub fn update_swing_time(&self, current_swing_duration: i32) {
        let mut state = self.state.lock();
        let swing = &mut state.swing;
        if swing.swinging {
            swing.swing_time += 1;
            if swing.swing_time >= current_swing_duration {
                swing.swing_time = 0;
                swing.swinging = false;
            }
        } else {
            swing.swing_time = 0;
        }

        swing.attack_anim = swing.swing_time as f32 / current_swing_duration as f32;
    }

    /// Returns vanilla `LivingEntity.absorptionAmount` for non-player living entities.
    #[must_use]
    pub fn absorption_amount(&self) -> f32 {
        self.state.lock().absorption_amount
    }

    /// Sets vanilla `LivingEntity.absorptionAmount` for non-player living entities.
    pub fn set_absorption_amount(&self, amount: f32) {
        self.state.lock().absorption_amount = amount.max(0.0);
    }

    /// Runs vanilla `LivingEntity.skipDropExperience`.
    pub fn skip_drop_experience(&self) {
        self.state.lock().skip_drop_experience = true;
    }

    /// Returns vanilla `LivingEntity.wasExperienceConsumed`.
    #[must_use]
    pub fn was_experience_consumed(&self) -> bool {
        self.state.lock().skip_drop_experience
    }

    /// Returns vanilla `LivingEntity.noActionTime`.
    #[must_use]
    pub fn no_action_time(&self) -> i32 {
        self.state.lock().no_action_time
    }

    /// Sets vanilla `LivingEntity.noActionTime`.
    pub fn set_no_action_time(&self, no_action_time: i32) {
        self.state.lock().no_action_time = no_action_time;
    }

    /// Increments vanilla `LivingEntity.noActionTime` by one tick.
    pub fn increment_no_action_time(&self) {
        self.state.lock().no_action_time += 1;
    }

    /// Refreshes transient item attribute modifiers for an equipment slot.
    pub fn refresh_equipment_attribute_modifiers(
        &self,
        slot: EquipmentSlot,
        item_stack: &ItemStack,
    ) {
        let slot_index = slot.index();
        let mut attributes = self.attributes.lock();
        let mut installed_modifiers = self.equipment_attribute_modifiers.lock();

        for key in installed_modifiers[slot_index].drain(..) {
            attributes.remove_modifier(key.attribute, &key.id);
        }

        if item_stack.is_empty() || item_stack.is_broken() {
            return;
        }

        let Some(modifiers) = item_stack.get_attribute_modifiers() else {
            return;
        };

        for entry in modifiers.for_slot(slot) {
            for (index, keys) in installed_modifiers.iter_mut().enumerate() {
                if index == slot_index {
                    continue;
                }
                keys.retain(|key| key.attribute.key != entry.attribute.key || key.id != entry.id);
            }

            attributes.remove_modifier(entry.attribute, &entry.id);
            if attributes.add_modifier(
                entry.attribute,
                AttributeModifier {
                    id: entry.id.clone(),
                    amount: entry.amount,
                    operation: entry.operation,
                },
                false,
            ) {
                installed_modifiers[slot_index].push(EquipmentAttributeModifierKey {
                    attribute: entry.attribute,
                    id: entry.id.clone(),
                });
            }
        }
    }

    /// Returns whether this living entity has an active vanilla mob effect.
    #[must_use]
    pub fn has_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.active_mob_effects.lock().contains_key(&effect)
    }

    /// Returns active vanilla mob-effect state.
    #[must_use]
    pub fn mob_effect(&self, effect: MobEffectRef) -> Option<ActiveMobEffect> {
        self.active_mob_effects.lock().get(&effect).cloned()
    }

    /// Returns all active vanilla mob effects.
    #[must_use]
    pub fn active_mob_effects(&self) -> Vec<ActiveMobEffect> {
        self.active_mob_effects.lock().values().cloned().collect()
    }

    /// Adds or updates active vanilla mob-effect state.
    pub fn add_mob_effect(&self, effect: MobEffectInstance) -> bool {
        let effect_key = effect.effect;
        let mut existing_effect = None;
        let mut changed_effect = None;
        {
            let mut effects = self.active_mob_effects.lock();
            if let Some(current) = effects.get_mut(&effect_key) {
                if current.update(effect) {
                    changed_effect = Some(current.clone());
                }
            } else {
                effects.insert(effect_key, effect.clone());
                existing_effect = Some(effect);
            }
        }

        if let Some(effect) = existing_effect {
            self.add_effect_attribute_modifiers(&effect);
            self.mark_effects_dirty();
            self.queue_mob_effect_sync(MobEffectSyncChange::Update {
                effect,
                blend_for_self: true,
            });
            return true;
        }

        if let Some(effect) = changed_effect {
            self.refresh_effect_attribute_modifiers(&effect);
            self.mark_effects_dirty();
            self.queue_mob_effect_sync(MobEffectSyncChange::Update {
                effect,
                blend_for_self: false,
            });
            return true;
        }

        false
    }

    /// Sets active vanilla mob-effect state.
    pub fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.add_mob_effect(MobEffectInstance::new(effect, amplifier));
    }

    /// Sets the presence of a vanilla mob effect.
    pub fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        if active {
            self.set_mob_effect(effect, 0);
        } else {
            self.remove_mob_effect(effect);
        }
    }

    /// Removes active vanilla mob-effect state.
    pub fn remove_mob_effect(&self, effect: MobEffectRef) -> bool {
        let removed = self.active_mob_effects.lock().remove(&effect);
        let Some(removed) = removed else {
            return false;
        };

        self.remove_effect_attribute_modifiers(removed.effect);
        self.mark_effects_dirty();
        self.queue_mob_effect_sync(MobEffectSyncChange::Remove { effect });
        true
    }

    /// Ticks active mob-effect durations and queues vanilla sync changes.
    pub fn tick_mob_effects(&self) {
        let mut removed = Vec::new();
        let mut updated = Vec::new();
        {
            let mut effects = self.active_mob_effects.lock();
            let effect_keys = effects.keys().copied().collect::<Vec<_>>();
            for effect_key in effect_keys {
                let Some(effect) = effects.get_mut(&effect_key) else {
                    continue;
                };
                match effect.tick_duration() {
                    MobEffectTickResult::Active { downgraded } => {
                        if downgraded || effect.duration() % 600 == 0 {
                            updated.push(effect.clone());
                        }
                    }
                    MobEffectTickResult::Expired => {
                        if let Some(effect) = effects.remove(&effect_key) {
                            removed.push(effect);
                        }
                    }
                }
            }
        }

        for effect in updated {
            self.refresh_effect_attribute_modifiers(&effect);
            self.mark_effects_dirty();
            self.queue_mob_effect_sync(MobEffectSyncChange::Update {
                effect,
                blend_for_self: false,
            });
        }

        for effect in removed {
            self.remove_effect_attribute_modifiers(effect.effect);
            self.mark_effects_dirty();
            self.queue_mob_effect_sync(MobEffectSyncChange::Remove {
                effect: effect.effect,
            });
        }
    }

    /// Drains pending mob-effect packet changes.
    pub fn drain_dirty_mob_effects(&self) -> Vec<MobEffectSyncChange> {
        self.dirty_mob_effects.lock().drain(..).collect()
    }

    /// Returns whether synchronized effect entity data should be recomputed.
    pub fn take_effects_dirty(&self) -> bool {
        let mut state = self.state.lock();
        let dirty = state.effects_dirty;
        state.effects_dirty = false;
        dirty
    }

    /// Builds the synchronized living effect particle/glow/invisibility state.
    pub fn mob_effect_display_state(
        &self,
        entity_effect_particle_type: i32,
    ) -> MobEffectDisplayState {
        let mut effects = self
            .active_mob_effects
            .lock()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        effects.sort_by_key(|effect| effect.effect.try_id().unwrap_or(usize::MAX));

        let particles = effects
            .iter()
            .filter(|effect| effect.is_visible())
            .map(|effect| {
                ParticleData::new(
                    entity_effect_particle_type,
                    ParticleOptions::Color {
                        color: effect.particle_color(),
                    },
                )
            })
            .collect();

        MobEffectDisplayState {
            particles: ParticleList { particles },
            ambient: !effects.is_empty() && effects.iter().all(MobEffectInstance::is_ambient),
            invisible: effects
                .iter()
                .any(|effect| effect.effect == vanilla_mob_effects::INVISIBILITY),
            glowing: effects
                .iter()
                .any(|effect| effect.effect == vanilla_mob_effects::GLOWING),
        }
    }

    fn add_effect_attribute_modifiers(&self, effect: &MobEffectInstance) {
        let mut attributes = self.attributes.lock();
        for modifier in effect.effect.attribute_modifiers {
            attributes.remove_modifier(modifier.attribute, &modifier.id);
            attributes.add_modifier(
                modifier.attribute,
                AttributeModifier {
                    id: modifier.id.clone(),
                    amount: modifier.amount * f64::from(effect.amplifier + 1),
                    operation: modifier.operation,
                },
                true,
            );
        }
    }

    fn refresh_effect_attribute_modifiers(&self, effect: &MobEffectInstance) {
        self.remove_effect_attribute_modifiers(effect.effect);
        self.add_effect_attribute_modifiers(effect);
    }

    fn remove_effect_attribute_modifiers(&self, effect: MobEffectRef) {
        let mut attributes = self.attributes.lock();
        for modifier in effect.attribute_modifiers {
            attributes.remove_modifier(modifier.attribute, &modifier.id);
        }
    }

    fn queue_mob_effect_sync(&self, change: MobEffectSyncChange) {
        self.dirty_mob_effects.lock().push(change);
    }

    fn mark_effects_dirty(&self) {
        self.state.lock().effects_dirty = true;
    }

    /// Gets the cached movement speed used by living movement code.
    #[inline]
    pub fn speed(&self) -> f32 {
        self.state.lock().speed
    }

    /// Sets the cached movement speed used by living movement code.
    #[inline]
    pub fn set_speed(&self, speed: f32) {
        self.state.lock().speed = speed;
    }

    /// Refreshes the cached movement speed from the `MOVEMENT_SPEED` attribute.
    pub fn refresh_speed_from_attributes(&self) {
        if let Some(speed) = self
            .attributes
            .lock()
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
        {
            self.state.lock().speed = speed as f32;
        }
    }

    /// Applies vanilla post-impulse movement validation grace.
    pub fn apply_post_impulse_grace_time(&self, ticks: i32) {
        let mut state = self.state.lock();
        state.current_impulse_context_reset_grace_time =
            state.current_impulse_context_reset_grace_time.max(ticks);
    }

    /// Mirrors vanilla `LivingEntity.setIgnoreFallDamageFromCurrentImpulse`.
    pub fn set_ignore_fall_damage_from_current_impulse(
        &self,
        ignore_fall_damage: bool,
        new_impulse_impact_pos: DVec3,
    ) {
        let mut state = self.state.lock();
        if ignore_fall_damage {
            state.current_impulse_context_reset_grace_time = state
                .current_impulse_context_reset_grace_time
                .max(POST_IMPULSE_GRACE_TICKS);
            state.current_impulse_impact_pos = Some(new_impulse_impact_pos);
        } else {
            state.current_impulse_context_reset_grace_time = 0;
        }
    }

    /// Returns vanilla `LivingEntity.currentImpulseImpactPos`.
    #[must_use]
    pub fn current_impulse_impact_pos(&self) -> Option<DVec3> {
        self.state.lock().current_impulse_impact_pos
    }

    /// Returns vanilla `LivingEntity.isIgnoringFallDamageFromCurrentImpulse`.
    #[must_use]
    pub fn is_ignoring_fall_damage_from_current_impulse(&self) -> bool {
        self.state.lock().current_impulse_impact_pos.is_some()
    }

    /// Mirrors vanilla `LivingEntity.tryResetCurrentImpulseContext`.
    pub fn try_reset_current_impulse_context(&self) {
        let mut state = self.state.lock();
        if state.current_impulse_context_reset_grace_time == 0 {
            state.current_impulse_impact_pos = None;
        }
    }

    /// Mirrors vanilla `LivingEntity.resetCurrentImpulseContext`.
    pub fn reset_current_impulse_context(&self) {
        let mut state = self.state.lock();
        state.current_impulse_context_reset_grace_time = 0;
        state.current_impulse_impact_pos = None;
    }

    /// Returns whether movement validation is inside post-impulse grace.
    #[must_use]
    pub fn is_in_post_impulse_grace_time(&self) -> bool {
        self.state.lock().current_impulse_context_reset_grace_time > 0
    }

    /// Decrements post-impulse grace once per living-entity tick.
    pub fn tick_post_impulse_grace_time(&self) {
        let mut state = self.state.lock();
        if state.current_impulse_context_reset_grace_time > 0 {
            state.current_impulse_context_reset_grace_time -= 1;
        }
    }

    /// Returns whether this living entity is currently fall flying.
    #[must_use]
    pub fn is_fall_flying(&self) -> bool {
        self.state.lock().fall_flying
    }

    /// Sets the vanilla living-entity fall-flying state.
    pub fn set_fall_flying(&self, fall_flying: bool) {
        self.state.lock().fall_flying = fall_flying;
    }

    /// Returns vanilla `LivingEntity.fallFlyTicks`.
    #[must_use]
    pub fn fall_flying_ticks(&self) -> i32 {
        self.state.lock().fall_flying_ticks
    }

    /// Ticks vanilla `LivingEntity.fallFlyTicks`.
    pub fn tick_fall_flying_state(&self, fall_flying: bool) {
        let mut state = self.state.lock();
        if fall_flying {
            state.fall_flying_ticks = state.fall_flying_ticks.wrapping_add(1);
        } else {
            state.fall_flying_ticks = 0;
        }
    }

    /// Returns whether this living entity is sprinting.
    #[must_use]
    pub fn is_sprinting(&self) -> bool {
        self.state.lock().sprinting
    }

    /// Sets the vanilla living-entity sprinting state and movement-speed modifier.
    pub fn set_sprinting(&self, sprinting: bool) {
        self.state.lock().sprinting = sprinting;

        let mut attributes = self.attributes.lock();
        if sprinting {
            attributes.add_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                AttributeModifier {
                    id: Identifier::vanilla_static("sprinting"),
                    amount: SPRINT_SPEED_MODIFIER_AMOUNT,
                    operation: AttributeModifierOperation::AddMultipliedTotal,
                },
                false,
            );
        } else {
            attributes.remove_modifier(
                vanilla_attributes::MOVEMENT_SPEED,
                &Identifier::vanilla_static("sprinting"),
            );
        }
    }

    /// Returns the bed position that makes this living entity sleeping.
    #[must_use]
    pub fn sleeping_pos(&self) -> Option<BlockPos> {
        self.state.lock().sleeping_pos
    }

    /// Sets the vanilla living-entity sleeping position.
    pub fn set_sleeping_pos(&self, bed_position: BlockPos) {
        self.state.lock().sleeping_pos = Some(bed_position);
    }

    /// Clears the vanilla living-entity sleeping position.
    pub fn clear_sleeping_pos(&self) {
        self.state.lock().sleeping_pos = None;
    }

    /// Returns whether this living entity has a sleeping position.
    #[must_use]
    pub fn is_sleeping(&self) -> bool {
        self.sleeping_pos().is_some()
    }

    /// Returns the last climbable block position this living entity touched.
    #[must_use]
    pub fn last_climbable_pos(&self) -> Option<BlockPos> {
        self.state.lock().last_climbable_pos
    }

    /// Records the last climbable block position this living entity touched.
    pub fn set_last_climbable_pos(&self, pos: BlockPos) {
        self.state.lock().last_climbable_pos = Some(pos);
    }

    /// Returns whether vanilla living travel should skip friction damping.
    #[must_use]
    pub fn should_discard_friction(&self) -> bool {
        self.state.lock().discard_friction
    }

    /// Sets whether vanilla living travel should skip friction damping.
    pub fn set_discard_friction(&self, discard_friction: bool) {
        self.state.lock().discard_friction = discard_friction;
    }

    /// Returns whether this living entity is applying jump input.
    #[must_use]
    pub fn is_jumping(&self) -> bool {
        self.state.lock().jumping
    }

    /// Sets whether this living entity is applying jump input.
    pub fn set_jumping(&self, jumping: bool) {
        self.state.lock().jumping = jumping;
    }

    /// Returns vanilla living travel input.
    #[must_use]
    pub fn travel_input(&self) -> LivingTravelInput {
        self.state.lock().travel_input
    }

    /// Sets vanilla living travel input.
    pub fn set_travel_input(&self, input: LivingTravelInput) {
        self.state.lock().travel_input = input;
    }

    /// Applies vanilla `LivingEntity.applyInput()` damping to travel input.
    pub fn dampen_travel_input(&self) {
        let mut state = self.state.lock();
        state.travel_input = state.travel_input.dampened();
    }

    /// Returns vanilla jump cooldown ticks.
    #[must_use]
    pub fn no_jump_delay(&self) -> i32 {
        self.state.lock().no_jump_delay
    }

    /// Sets vanilla jump cooldown ticks.
    pub fn set_no_jump_delay(&self, ticks: i32) {
        self.state.lock().no_jump_delay = ticks;
    }

    /// Decrements vanilla jump cooldown once per living AI step.
    pub fn tick_no_jump_delay(&self) {
        let mut state = self.state.lock();
        if state.no_jump_delay > 0 {
            state.no_jump_delay -= 1;
        }
    }

    /// Calculates vanilla living-entity fall damage.
    #[must_use]
    pub fn calculate_fall_damage(
        fall_distance: f64,
        damage_modifier: f32,
        safe_fall_distance: f64,
        fall_damage_multiplier: f64,
    ) -> i32 {
        ((fall_distance + 1.0e-6 - safe_fall_distance)
            * f64::from(damage_modifier)
            * fall_damage_multiplier)
            .floor() as i32
    }

    /// Decrements remaining invulnerability ticks by one if any are active.
    pub fn decrement_invulnerable_time(&self) {
        let mut state = self.state.lock();
        if state.invulnerable_time > 0 {
            state.invulnerable_time -= 1;
        }
    }

    /// Applies vanilla hurt cooldown bookkeeping.
    ///
    /// Returns `None` when damage should be ignored because death was already
    /// processed or the amount did not exceed the active invulnerability frame.
    pub fn apply_damage_cooldown(
        &self,
        amount: f32,
        bypasses_cooldown: bool,
    ) -> Option<(bool, f32)> {
        let mut state = self.state.lock();
        if state.death_processed {
            return None;
        }

        if state.invulnerable_time > 10 && !bypasses_cooldown {
            if amount <= state.last_hurt {
                return None;
            }
            let effective = amount - state.last_hurt;
            state.last_hurt = amount;
            Some((false, effective))
        } else {
            state.last_hurt = amount;
            state.invulnerable_time = 20;
            Some((true, amount))
        }
    }

    /// Records vanilla `LivingEntity.lastDamageSource` after successful damage.
    pub fn record_last_damage_source(&self, source: &DamageSource, game_time: i64) {
        let mut state = self.state.lock();
        state.last_damage_source = Some(source.clone());
        state.last_damage_stamp = game_time;
    }

    /// Returns vanilla `LivingEntity.getLastDamageSource()`.
    pub fn last_damage_source(&self, game_time: i64) -> Option<DamageSource> {
        let mut state = self.state.lock();
        if game_time - state.last_damage_stamp > 40 {
            state.last_damage_source = None;
        }
        state.last_damage_source.clone()
    }

    /// Sets vanilla `LivingEntity.lastHurtByPlayer` and memory time.
    pub fn set_last_hurt_by_player(&self, player_uuid: Uuid, time_to_remember: i32) {
        let mut state = self.state.lock();
        state.last_hurt_by_player = Some(player_uuid);
        state.last_hurt_by_player_memory_time = time_to_remember;
    }

    /// Returns vanilla `LivingEntity.lastHurtByPlayerMemoryTime`.
    #[must_use]
    pub fn last_hurt_by_player_memory_time(&self) -> i32 {
        self.state.lock().last_hurt_by_player_memory_time
    }

    /// Returns the remembered player UUID, if present.
    #[must_use]
    pub fn last_hurt_by_player_uuid(&self) -> Option<Uuid> {
        self.state.lock().last_hurt_by_player
    }

    /// Returns vanilla `LivingEntity.lastHurtByMob`, if still resolvable.
    #[must_use]
    pub fn last_hurt_by_mob(&self) -> Option<SharedEntity> {
        let mut state = self.state.lock();
        living_entity_from_weak(&mut state.last_hurt_by_mob)
    }

    /// Returns vanilla `LivingEntity.lastHurtByMobTimestamp`.
    #[must_use]
    pub fn last_hurt_by_mob_timestamp(&self) -> i32 {
        self.state.lock().last_hurt_by_mob_timestamp
    }

    /// Sets vanilla `LivingEntity.lastHurtByMob` and timestamp.
    pub fn set_last_hurt_by_mob(&self, target: Option<&SharedEntity>, tick_count: i32) {
        let mut state = self.state.lock();
        state.last_hurt_by_mob = weak_living_entity(target);
        state.last_hurt_by_mob_timestamp = tick_count;
    }

    /// Returns vanilla `LivingEntity.lastHurtMob`, if still resolvable.
    #[must_use]
    pub fn last_hurt_mob(&self) -> Option<SharedEntity> {
        let mut state = self.state.lock();
        living_entity_from_weak(&mut state.last_hurt_mob)
    }

    /// Returns vanilla `LivingEntity.lastHurtMobTimestamp`.
    #[must_use]
    pub fn last_hurt_mob_timestamp(&self) -> i32 {
        self.state.lock().last_hurt_mob_timestamp
    }

    /// Sets vanilla `LivingEntity.lastHurtMob` and timestamp.
    pub fn set_last_hurt_mob(&self, target: Option<&SharedEntity>, tick_count: i32) {
        let mut state = self.state.lock();
        state.last_hurt_mob = weak_living_entity(target);
        state.last_hurt_mob_timestamp = tick_count;
    }

    /// Ticks vanilla last-hurt-by-player memory.
    pub fn tick_last_hurt_by_player_memory(&self) {
        let mut state = self.state.lock();
        if state.last_hurt_by_player_memory_time > 0 {
            state.last_hurt_by_player_memory_time -= 1;
        } else {
            state.last_hurt_by_player = None;
        }
    }

    /// Ticks vanilla living combat-memory cleanup.
    pub fn tick_living_combat_memory(&self, tick_count: i32) {
        if self
            .last_hurt_mob()
            .is_some_and(|target| living_is_dead(&target))
        {
            self.set_last_hurt_mob(None, tick_count);
        }

        let Some(hurt_by) = self.last_hurt_by_mob() else {
            return;
        };
        if living_is_dead(&hurt_by) || tick_count - self.last_hurt_by_mob_timestamp() > 100 {
            self.set_last_hurt_by_mob(None, tick_count);
        }
    }

    /// Marks death side effects as processed.
    ///
    /// Returns `false` if they were already processed.
    pub fn mark_death_processed(&self) -> bool {
        let mut state = self.state.lock();
        if state.death_processed {
            return false;
        }
        state.death_processed = true;
        true
    }

    /// Increments death animation time by 1 and returns the new value.
    #[inline]
    pub fn increment_death_time(&self) -> i32 {
        let mut state = self.state.lock();
        state.death_time += 1;
        state.death_time
    }

    /// Resets all death-related state back to alive defaults.
    #[inline]
    pub fn reset_death_state(&self) {
        self.state.lock().reset_death_state();
    }

    /// Resets state that vanilla gets from constructing a fresh living player for death respawn.
    pub fn reset_for_player_respawn(&self) {
        self.set_sprinting(false);
        let removed_effects = {
            let mut effects = self.active_mob_effects.lock();
            let removed_effects = effects.keys().copied().collect::<Vec<_>>();
            effects.clear();
            removed_effects
        };

        for effect in removed_effects.iter().copied() {
            self.remove_effect_attribute_modifiers(effect);
        }

        {
            let mut dirty_effects = self.dirty_mob_effects.lock();
            dirty_effects.clear();
            dirty_effects.extend(
                removed_effects
                    .into_iter()
                    .map(|effect| MobEffectSyncChange::Remove { effect }),
            );
        }

        let speed = self
            .attributes
            .lock()
            .required_value(vanilla_attributes::MOVEMENT_SPEED) as f32;

        let mut state = self.state.lock();
        *state = LivingEntityState::new(speed);
        state.effects_dirty = true;
    }
}

fn weak_living_entity(target: Option<&SharedEntity>) -> Option<WeakEntity> {
    let target = target?;
    target.is_living_entity().then(|| Arc::downgrade(target))
}

fn living_entity_from_weak(entity: &mut Option<WeakEntity>) -> Option<SharedEntity> {
    let Some(upgraded) = entity.as_ref().and_then(WeakEntity::upgrade) else {
        *entity = None;
        return None;
    };
    if !upgraded.is_living_entity() {
        *entity = None;
        return None;
    }
    Some(upgraded)
}

fn living_is_dead(entity: &SharedEntity) -> bool {
    entity
        .as_living_entity()
        .is_none_or(|living| !LivingEntity::is_alive(living))
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        item_stack::ItemStack, test_support::init_test_registry, vanilla_attributes,
        vanilla_damage_types, vanilla_entities, vanilla_entity_data::PlayerEntityData,
        vanilla_items, vanilla_mob_effects,
    };
    use steel_utils::{BlockPos, types::InteractionHand};

    use crate::entity::damage::DamageSource;
    use crate::inventory::equipment::EquipmentSlot;

    use super::{
        ActiveMobEffect, DEFAULT_SWING_DURATION, LivingEntityBase, LivingTravelInput,
        MobEffectInstance, MobEffectSyncChange, POST_IMPULSE_GRACE_TICKS,
    };

    #[test]
    fn living_constructor_initializes_health_from_max_health() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let mut entity_data = PlayerEntityData::new();

        assert_eq!(
            entity_data.living_entity().health.get().to_bits(),
            1.0_f32.to_bits()
        );

        base.initialize_synced_data(&mut entity_data);

        assert_eq!(
            entity_data.living_entity().health.get().to_bits(),
            (vanilla_attributes::MAX_HEALTH.default_value as f32).to_bits()
        );
    }

    #[test]
    fn fall_damage_starts_above_safe_fall_distance() {
        assert_eq!(
            LivingEntityBase::calculate_fall_damage(3.0, 1.0, 3.0, 1.0),
            0
        );
        assert_eq!(
            LivingEntityBase::calculate_fall_damage(4.0, 1.0, 3.0, 1.0),
            1
        );
    }

    #[test]
    fn last_damage_source_expires_after_vanilla_window() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PIG);
        let source = DamageSource::environment(&vanilla_damage_types::GENERIC);

        assert!(base.last_damage_source(0).is_none());

        base.record_last_damage_source(&source, 10);

        let last_source = base
            .last_damage_source(50)
            .expect("last damage source should remain valid for 40 ticks");
        assert_eq!(last_source.damage_type, &vanilla_damage_types::GENERIC);
        assert!(base.last_damage_source(51).is_none());
    }

    #[test]
    fn last_hurt_by_player_memory_ticks_down_then_clears_reference() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PIG);
        let player_uuid = uuid::Uuid::from_u128(7);

        base.set_last_hurt_by_player(player_uuid, 2);

        assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
        assert_eq!(base.last_hurt_by_player_memory_time(), 2);

        base.tick_last_hurt_by_player_memory();

        assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
        assert_eq!(base.last_hurt_by_player_memory_time(), 1);

        base.tick_last_hurt_by_player_memory();

        assert_eq!(base.last_hurt_by_player_uuid(), Some(player_uuid));
        assert_eq!(base.last_hurt_by_player_memory_time(), 0);

        base.tick_last_hurt_by_player_memory();

        assert!(base.last_hurt_by_player_uuid().is_none());
        assert_eq!(base.last_hurt_by_player_memory_time(), 0);
    }

    #[test]
    fn fall_damage_applies_block_and_attribute_multipliers() {
        assert_eq!(
            LivingEntityBase::calculate_fall_damage(8.0, 0.5, 3.0, 2.0),
            5
        );
        assert_eq!(
            LivingEntityBase::calculate_fall_damage(8.0, 0.2, 3.0, 1.0),
            1
        );
    }

    #[test]
    fn post_impulse_grace_counts_down_by_tick() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.apply_post_impulse_grace_time(2);

        assert!(base.is_in_post_impulse_grace_time());
        base.tick_post_impulse_grace_time();
        assert!(base.is_in_post_impulse_grace_time());
        base.tick_post_impulse_grace_time();
        assert!(!base.is_in_post_impulse_grace_time());
    }

    #[test]
    fn post_impulse_grace_keeps_larger_existing_window() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.apply_post_impulse_grace_time(5);
        base.apply_post_impulse_grace_time(2);

        for _ in 0..4 {
            base.tick_post_impulse_grace_time();
            assert!(base.is_in_post_impulse_grace_time());
        }

        base.tick_post_impulse_grace_time();
        assert!(!base.is_in_post_impulse_grace_time());
    }

    #[test]
    fn current_impulse_context_tracks_fall_damage_impact_position() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let impact_pos = DVec3::new(1.0, 72.0, -3.0);

        base.set_ignore_fall_damage_from_current_impulse(true, impact_pos);

        assert!(base.is_ignoring_fall_damage_from_current_impulse());
        assert_eq!(base.current_impulse_impact_pos(), Some(impact_pos));
        assert!(base.is_in_post_impulse_grace_time());
    }

    #[test]
    fn current_impulse_context_resets_after_grace_window() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.set_ignore_fall_damage_from_current_impulse(true, DVec3::new(0.0, 72.0, 0.0));
        base.apply_post_impulse_grace_time(1);
        base.try_reset_current_impulse_context();
        assert!(base.is_ignoring_fall_damage_from_current_impulse());

        for _ in 0..POST_IMPULSE_GRACE_TICKS {
            base.tick_post_impulse_grace_time();
        }
        base.try_reset_current_impulse_context();

        assert!(!base.is_ignoring_fall_damage_from_current_impulse());
        assert_eq!(base.current_impulse_impact_pos(), None);
    }

    #[test]
    fn fall_flying_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert!(!base.is_fall_flying());
        base.set_fall_flying(true);
        assert!(base.is_fall_flying());
        base.set_fall_flying(false);
        assert!(!base.is_fall_flying());
    }

    #[test]
    fn fall_flying_ticks_are_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert_eq!(base.fall_flying_ticks(), 0);
        base.tick_fall_flying_state(true);
        base.tick_fall_flying_state(true);
        assert_eq!(base.fall_flying_ticks(), 2);
        base.tick_fall_flying_state(false);
        assert_eq!(base.fall_flying_ticks(), 0);
    }

    #[test]
    fn living_rotation_is_base_tick_snapshot_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PIG);

        base.set_y_body_rot(30.0);
        base.set_y_head_rot(45.0);
        let rotation = base.rotation_state();
        assert_eq!(rotation.y_body_rot().to_bits(), 30.0_f32.to_bits());
        assert_eq!(rotation.y_body_rot_o().to_bits(), 0.0_f32.to_bits());
        assert_eq!(rotation.y_head_rot().to_bits(), 45.0_f32.to_bits());
        assert_eq!(rotation.y_head_rot_o().to_bits(), 0.0_f32.to_bits());

        base.advance_rotation_for_base_tick();
        let rotation = base.rotation_state();
        assert_eq!(rotation.y_body_rot_o().to_bits(), 30.0_f32.to_bits());
        assert_eq!(rotation.y_head_rot_o().to_bits(), 45.0_f32.to_bits());

        base.set_y_body_rot(60.0);
        base.set_y_head_rot(75.0);
        let rotation = base.rotation_state();
        assert_eq!(rotation.y_body_rot().to_bits(), 60.0_f32.to_bits());
        assert_eq!(rotation.y_body_rot_o().to_bits(), 30.0_f32.to_bits());
        assert_eq!(rotation.y_head_rot().to_bits(), 75.0_f32.to_bits());
        assert_eq!(rotation.y_head_rot_o().to_bits(), 45.0_f32.to_bits());
    }

    #[test]
    fn living_swing_uses_vanilla_restart_gate() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PIG);

        assert!(base.start_swing(InteractionHand::MainHand, DEFAULT_SWING_DURATION));
        let state = base.swing_state();
        assert!(state.swinging());
        assert_eq!(state.swinging_arm(), Some(InteractionHand::MainHand));
        assert_eq!(state.swing_time(), -1);

        base.update_swing_time(DEFAULT_SWING_DURATION);
        assert!(!base.start_swing(InteractionHand::OffHand, DEFAULT_SWING_DURATION));
        assert_eq!(
            base.swing_state().swinging_arm(),
            Some(InteractionHand::MainHand)
        );

        for _ in 0..3 {
            base.update_swing_time(DEFAULT_SWING_DURATION);
        }
        assert!(base.start_swing(InteractionHand::OffHand, DEFAULT_SWING_DURATION));
        let state = base.swing_state();
        assert_eq!(state.swinging_arm(), Some(InteractionHand::OffHand));
        assert_eq!(state.swing_time(), -1);
    }

    #[test]
    fn living_swing_time_updates_attack_animation() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PIG);

        assert!(base.start_swing(InteractionHand::MainHand, DEFAULT_SWING_DURATION));
        base.update_swing_time(DEFAULT_SWING_DURATION);
        base.update_swing_time(DEFAULT_SWING_DURATION);
        let state = base.swing_state();
        assert!(state.swinging());
        assert_eq!(state.swing_time(), 1);
        assert_eq!(
            state.attack_anim().to_bits(),
            (1.0_f32 / DEFAULT_SWING_DURATION as f32).to_bits()
        );

        base.advance_attack_animation_for_base_tick();
        assert_eq!(
            base.swing_state().old_attack_anim().to_bits(),
            (1.0_f32 / DEFAULT_SWING_DURATION as f32).to_bits()
        );

        for _ in 0..5 {
            base.update_swing_time(DEFAULT_SWING_DURATION);
        }
        let state = base.swing_state();
        assert!(!state.swinging());
        assert_eq!(state.swing_time(), 0);
        assert_eq!(state.attack_anim().to_bits(), 0.0_f32.to_bits());
    }

    #[test]
    fn equipment_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert!(base.equipment().lock().is_empty());

        base.equipment().lock().set(
            EquipmentSlot::Chest,
            ItemStack::new(&vanilla_items::ITEMS.elytra),
        );

        assert!(
            base.equipment()
                .lock()
                .get_ref(EquipmentSlot::Chest)
                .is(&vanilla_items::ITEMS.elytra)
        );
    }

    #[test]
    fn sprinting_is_living_entity_state_and_speed_modifier() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
        let base_speed = base
            .attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed");

        assert!(!base.is_sprinting());
        base.set_sprinting(true);
        assert!(base.is_sprinting());
        assert!(
            base.attributes()
                .lock()
                .get_value(movement_speed)
                .expect("player should have movement speed")
                > base_speed
        );

        base.set_sprinting(false);
        assert!(!base.is_sprinting());
        assert_eq!(
            base.attributes()
                .lock()
                .get_value(movement_speed)
                .expect("player should have movement speed")
                .to_bits(),
            base_speed.to_bits()
        );
    }

    #[test]
    fn active_mob_effect_presence_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
        base.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, true);
        assert!(base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
        assert_eq!(
            base.mob_effect(vanilla_mob_effects::DOLPHINS_GRACE),
            Some(ActiveMobEffect::new(vanilla_mob_effects::DOLPHINS_GRACE, 0))
        );
        base.set_mob_effect_active(vanilla_mob_effects::DOLPHINS_GRACE, false);
        assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
    }

    #[test]
    fn active_mob_effect_amplifier_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.set_mob_effect(vanilla_mob_effects::JUMP_BOOST, 2);

        assert_eq!(
            base.mob_effect(vanilla_mob_effects::JUMP_BOOST),
            Some(ActiveMobEffect::new(vanilla_mob_effects::JUMP_BOOST, 2))
        );
    }

    #[test]
    fn mob_effect_attribute_modifiers_use_extracted_vanilla_data() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
        let base_speed = base
            .attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed");
        let speed_modifier = &vanilla_mob_effects::SPEED.attribute_modifiers[0];

        assert_eq!(speed_modifier.attribute.key, movement_speed.key);
        assert!(base.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::SPEED,
            200,
            1
        )));

        let boosted_speed = base
            .attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed");
        let expected = base_speed * (1.0 + speed_modifier.amount * 2.0);
        assert!((boosted_speed - expected).abs() < f64::EPSILON);

        assert!(base.remove_mob_effect(vanilla_mob_effects::SPEED));
        assert_eq!(
            base.attributes()
                .lock()
                .get_value(movement_speed)
                .expect("player should have movement speed")
                .to_bits(),
            base_speed.to_bits()
        );
    }

    #[test]
    fn player_respawn_reset_clears_living_runtime_and_effect_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let movement_speed = vanilla_attributes::MOVEMENT_SPEED;
        let base_speed = base
            .attributes()
            .lock()
            .get_value(movement_speed)
            .expect("player should have movement speed");

        base.set_sprinting(true);
        base.set_sleeping_pos(BlockPos::new(1, 64, 1));
        base.set_fall_flying(true);
        base.tick_fall_flying_state(true);
        base.set_absorption_amount(4.0);
        base.skip_drop_experience();
        base.set_no_action_time(80);
        base.set_last_hurt_by_player(uuid::Uuid::from_u128(9), 100);
        base.record_last_damage_source(
            &DamageSource::environment(&vanilla_damage_types::GENERIC),
            7,
        );
        assert!(base.apply_damage_cooldown(4.0, false).is_some());
        assert!(base.mark_death_processed());
        assert_eq!(base.increment_death_time(), 1);
        base.set_mob_effect(vanilla_mob_effects::SPEED, 1);
        base.set_mob_effect(vanilla_mob_effects::INVISIBILITY, 0);
        base.drain_dirty_mob_effects();

        base.reset_for_player_respawn();

        assert!(!base.is_sprinting());
        assert_eq!(base.sleeping_pos(), None);
        assert!(!base.is_fall_flying());
        assert_eq!(base.fall_flying_ticks(), 0);
        assert_eq!(base.absorption_amount().to_bits(), 0.0_f32.to_bits());
        assert!(!base.was_experience_consumed());
        assert_eq!(base.no_action_time(), 0);
        assert!(base.last_hurt_by_player_uuid().is_none());
        assert!(base.last_damage_source(7).is_none());
        assert!(!base.has_mob_effect(vanilla_mob_effects::SPEED));
        assert!(!base.has_mob_effect(vanilla_mob_effects::INVISIBILITY));
        assert_eq!(
            base.attributes()
                .lock()
                .get_value(movement_speed)
                .expect("player should have movement speed")
                .to_bits(),
            base_speed.to_bits()
        );

        let state = base.state.lock();
        assert!(!state.death_processed);
        assert_eq!(state.death_time, 0);
        assert_eq!(state.last_hurt.to_bits(), 0.0_f32.to_bits());
        drop(state);

        let changes = base.drain_dirty_mob_effects();
        assert!(changes.contains(&MobEffectSyncChange::Remove {
            effect: vanilla_mob_effects::SPEED
        }));
        assert!(changes.contains(&MobEffectSyncChange::Remove {
            effect: vanilla_mob_effects::INVISIBILITY
        }));
        assert!(base.take_effects_dirty());
    }

    #[test]
    fn mob_effect_duration_tick_removes_expired_effect() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::DOLPHINS_GRACE,
            1,
            0,
        ));
        base.drain_dirty_mob_effects();

        base.tick_mob_effects();

        assert!(!base.has_mob_effect(vanilla_mob_effects::DOLPHINS_GRACE));
        assert_eq!(
            base.drain_dirty_mob_effects(),
            vec![MobEffectSyncChange::Remove {
                effect: vanilla_mob_effects::DOLPHINS_GRACE
            }]
        );
    }

    #[test]
    fn stronger_shorter_effect_downgrades_to_hidden_effect() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        base.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::SPEED,
            10,
            0,
        ));
        base.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::SPEED,
            2,
            1,
        ));
        base.drain_dirty_mob_effects();

        base.tick_mob_effects();
        base.tick_mob_effects();

        let effect = base
            .mob_effect(vanilla_mob_effects::SPEED)
            .expect("speed should downgrade to hidden effect");
        assert_eq!(effect.amplifier(), 0);
        assert_eq!(effect.duration(), 8);
        assert_eq!(
            base.drain_dirty_mob_effects(),
            vec![MobEffectSyncChange::Update {
                effect,
                blend_for_self: false,
            }]
        );
    }

    #[test]
    fn sleeping_uses_living_entity_sleeping_position() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let bed_pos = BlockPos::new(12, 64, -4);

        assert!(!base.is_sleeping());
        assert_eq!(base.sleeping_pos(), None);

        base.set_sleeping_pos(bed_pos);
        assert!(base.is_sleeping());
        assert_eq!(base.sleeping_pos(), Some(bed_pos));

        base.clear_sleeping_pos();
        assert!(!base.is_sleeping());
        assert_eq!(base.sleeping_pos(), None);
    }

    #[test]
    fn last_climbable_pos_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);
        let climbable_pos = BlockPos::new(-5, 72, 3);

        assert_eq!(base.last_climbable_pos(), None);
        base.set_last_climbable_pos(climbable_pos);
        assert_eq!(base.last_climbable_pos(), Some(climbable_pos));
    }

    #[test]
    fn discard_friction_is_living_entity_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert!(!base.should_discard_friction());
        base.set_discard_friction(true);
        assert!(base.should_discard_friction());
        base.set_discard_friction(false);
        assert!(!base.should_discard_friction());
    }

    #[test]
    fn living_travel_input_is_shared_living_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert_eq!(base.travel_input(), LivingTravelInput::ZERO);
        base.set_travel_input(LivingTravelInput::new(1.0, 0.5, -1.0));
        assert_eq!(base.travel_input(), LivingTravelInput::new(1.0, 0.5, -1.0));

        base.dampen_travel_input();
        assert_eq!(
            base.travel_input(),
            LivingTravelInput::new(0.98, 0.5, -0.98)
        );
    }

    #[test]
    fn jumping_and_jump_delay_are_shared_living_state() {
        init_test_registry();
        let base = LivingEntityBase::new(&vanilla_entities::PLAYER);

        assert!(!base.is_jumping());
        base.set_jumping(true);
        assert!(base.is_jumping());

        assert_eq!(base.no_jump_delay(), 0);
        base.set_no_jump_delay(2);
        base.tick_no_jump_delay();
        assert_eq!(base.no_jump_delay(), 1);
        base.tick_no_jump_delay();
        base.tick_no_jump_delay();
        assert_eq!(base.no_jump_delay(), 0);
    }
}
