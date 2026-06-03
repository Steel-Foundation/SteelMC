//! Shared fields for all living entities.
//!
//! Mirrors the runtime fields that vanilla defines on `LivingEntity` (and
//! `Entity` for `invulnerableTime`). Entities that implement `LivingEntity`
//! embed this struct and expose it via `LivingEntity::living_base()`, just like
//! `EntityBase` is used for core `Entity` fields.

use steel_registry::entity_type::EntityTypeRef;
use steel_registry::vanilla_attributes;
use steel_utils::locks::SyncMutex;
use steel_utils::{BlockPos, Identifier};

use crate::entity::attribute::{AttributeMap, AttributeModifier, AttributeModifierOperation};

/// Duration in ticks of the death animation before entity removal.
pub const DEATH_DURATION: i32 = 20;
const SPRINT_SPEED_MODIFIER_AMOUNT: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq)]
struct LivingEntityState {
    death_processed: bool,
    invulnerable_time: i32,
    last_hurt: f32,
    death_time: i32,
    speed: f32,
    current_impulse_context_reset_grace_time: i32,
    fall_flying: bool,
    sprinting: bool,
    sleeping_pos: Option<BlockPos>,
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
            sprinting: false,
            sleeping_pos: None,
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
        let speed = attributes
            .get_value(vanilla_attributes::MOVEMENT_SPEED)
            .unwrap_or(0.1) as f32;

        Self {
            state: SyncMutex::new(LivingEntityState::new(speed)),
            attributes: SyncMutex::new(attributes),
        }
    }

    /// Returns this entity's attribute map.
    #[inline]
    pub const fn attributes(&self) -> &SyncMutex<AttributeMap> {
        &self.attributes
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
    use steel_registry::{test_support::init_test_registry, vanilla_attributes, vanilla_entities};
    use steel_utils::BlockPos;

    use super::LivingEntityBase;

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
}
