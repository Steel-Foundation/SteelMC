/// Vanilla base fire and freezing state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntityFireFreezeState {
    pub(super) remaining_fire_ticks: i32,
    pub(super) ticks_frozen: i32,
    pub(super) is_in_powder_snow: bool,
    pub(super) was_in_powder_snow: bool,
    pub(super) has_visual_fire: bool,
}

impl EntityFireFreezeState {
    /// Creates default vanilla fire/freeze state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            remaining_fire_ticks: 0,
            ticks_frozen: 0,
            is_in_powder_snow: false,
            was_in_powder_snow: false,
            has_visual_fire: false,
        }
    }

    /// Creates fire/freeze state restored from persistent data.
    #[must_use]
    pub const fn from_parts(
        remaining_fire_ticks: i32,
        ticks_frozen: i32,
        is_in_powder_snow: bool,
        was_in_powder_snow: bool,
        has_visual_fire: bool,
    ) -> Self {
        Self {
            remaining_fire_ticks,
            ticks_frozen,
            is_in_powder_snow,
            was_in_powder_snow,
            has_visual_fire,
        }
    }

    /// Returns vanilla `remainingFireTicks`.
    #[must_use]
    pub const fn remaining_fire_ticks(self) -> i32 {
        self.remaining_fire_ticks
    }

    /// Returns synchronized vanilla `TicksFrozen`.
    #[must_use]
    pub const fn ticks_frozen(self) -> i32 {
        self.ticks_frozen
    }

    /// Returns whether this entity touched powder snow during the current tick.
    #[must_use]
    pub const fn is_in_powder_snow(self) -> bool {
        self.is_in_powder_snow
    }

    /// Returns whether this entity touched powder snow during the previous tick.
    #[must_use]
    pub const fn was_in_powder_snow(self) -> bool {
        self.was_in_powder_snow
    }

    /// Returns vanilla `hasVisualFire`.
    #[must_use]
    pub const fn has_visual_fire(self) -> bool {
        self.has_visual_fire
    }

    /// Returns whether the entity has any frozen ticks.
    #[must_use]
    pub const fn is_freezing(self) -> bool {
        self.ticks_frozen > 0
    }

    /// Returns whether the entity has reached vanilla full-freeze duration.
    #[must_use]
    pub const fn is_fully_frozen(self, ticks_required_to_freeze: i32) -> bool {
        self.ticks_frozen >= ticks_required_to_freeze
    }
}

impl Default for EntityFireFreezeState {
    fn default() -> Self {
        Self::new()
    }
}
