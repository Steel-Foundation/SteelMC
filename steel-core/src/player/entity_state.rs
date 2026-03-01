//! Core entity state flags for a player.
//!
//! Groups the boolean/simple state flags that describe what the player is
//! physically doing: sleeping, gliding, on the ground, sneaking, sprinting.

/// Physical state flags for a player entity.
pub struct EntityState {
    /// Whether the player is currently sleeping in a bed.
    pub sleeping: bool,
    /// Whether the player is currently fall flying (elytra gliding).
    pub fall_flying: bool,
    /// Whether the player is on the ground.
    pub on_ground: bool,
    /// Whether the player is sneaking (shift key down).
    pub crouching: bool,
    /// Whether the player is sprinting.
    pub sprinting: bool,
}

impl EntityState {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sleeping: false,
            fall_flying: false,
            on_ground: false,
            crouching: false,
            sprinting: false,
        }
    }
}
