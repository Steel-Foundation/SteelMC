//! Shared fields for all living entities.
//!
//! Mirrors the runtime fields that vanilla defines on `LivingEntity` (and
//! `Entity` for `invulnerableTime`). Entities that implement `LivingEntity`
//! embed this struct and expose it via `LivingEntity::living_base()`, just like
//! `EntityBase` is used for core `Entity` fields.

use rustc_hash::FxHashMap;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::mob_effect::MobEffectRef;
use steel_registry::vanilla_attributes;
use steel_registry::vanilla_entity_data::VanillaLivingEntityData;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Identifier};

use crate::entity::attribute::{AttributeMap, AttributeModifier, AttributeModifierOperation};
use crate::inventory::equipment::EntityEquipment;

/// Duration in ticks of the death animation before entity removal.
pub const DEATH_DURATION: i32 = 20;
const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;

/// Runtime mob-effect state currently needed by living physics.
///
/// TODO: Extend this into full vanilla `MobEffectInstance` state with duration,
/// ambience, visibility, hidden effects, attribute modifiers, ticking, and sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveMobEffect {
    effect: MobEffectRef,
    amplifier: i32,
}

impl ActiveMobEffect {
    /// Creates active mob-effect state.
    #[must_use]
    pub const fn new(effect: MobEffectRef, amplifier: i32) -> Self {
        Self { effect, amplifier }
    }

    /// Returns the mob effect.
    #[must_use]
    pub const fn effect(self) -> MobEffectRef {
        self.effect
    }

    /// Returns vanilla `MobEffectInstance.getAmplifier()`.
    #[must_use]
    pub const fn amplifier(self) -> i32 {
        self.amplifier
    }
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

#[derive(Debug, Clone, Copy, PartialEq)]
struct LivingEntityState {
    death_processed: bool,
    invulnerable_time: i32,
    last_hurt: f32,
    death_time: i32,
    speed: f32,
    current_impulse_context_reset_grace_time: i32,
    fall_flying: bool,
    fall_flying_ticks: i32,
    sprinting: bool,
    sleeping_pos: Option<BlockPos>,
    last_climbable_pos: Option<BlockPos>,
    discard_friction: bool,
    jumping: bool,
    travel_input: LivingTravelInput,
    no_jump_delay: i32,
}

impl LivingEntityState {
    const fn new(speed: f32) -> Self {
        Self {
            death_processed: false,
            invulnerable_time: 0,
            last_hurt: 0.0,
            death_time: 0,
            speed,
            current_impulse_context_reset_grace_time: 0,
            fall_flying: false,
            fall_flying_ticks: 0,
            sprinting: false,
            sleeping_pos: None,
            last_climbable_pos: None,
            discard_friction: false,
            jumping: false,
            travel_input: LivingTravelInput::ZERO,
            no_jump_delay: 0,
        }
    }

    const fn reset_death_state(&mut self) {
        self.death_processed = false;
        self.death_time = 0;
        self.invulnerable_time = 0;
        self.last_hurt = 0.0;
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
    equipment: SyncMutex<EntityEquipment>,
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
            equipment: SyncMutex::new(EntityEquipment::new()),
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

    /// Returns whether this living entity has an active vanilla mob effect.
    #[must_use]
    pub fn has_mob_effect(&self, effect: MobEffectRef) -> bool {
        self.active_mob_effects.lock().contains_key(&effect)
    }

    /// Returns active vanilla mob-effect state.
    #[must_use]
    pub fn mob_effect(&self, effect: MobEffectRef) -> Option<ActiveMobEffect> {
        self.active_mob_effects.lock().get(&effect).copied()
    }

    /// Sets active vanilla mob-effect state.
    pub fn set_mob_effect(&self, effect: MobEffectRef, amplifier: i32) {
        self.active_mob_effects
            .lock()
            .insert(effect, ActiveMobEffect::new(effect, amplifier));
    }

    /// Sets the presence of a vanilla mob effect.
    pub fn set_mob_effect_active(&self, effect: MobEffectRef, active: bool) {
        let mut effects = self.active_mob_effects.lock();
        if active {
            effects.insert(effect, ActiveMobEffect::new(effect, 0));
        } else {
            effects.remove(&effect);
        }
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
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        item_stack::ItemStack, test_support::init_test_registry, vanilla_attributes,
        vanilla_entities, vanilla_entity_data::PlayerEntityData, vanilla_items,
        vanilla_mob_effects,
    };
    use steel_utils::BlockPos;

    use crate::inventory::equipment::EquipmentSlot;

    use super::{ActiveMobEffect, LivingEntityBase, LivingTravelInput};

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
