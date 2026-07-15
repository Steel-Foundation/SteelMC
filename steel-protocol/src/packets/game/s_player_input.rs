use steel_macros::{ReadFrom, ServerPacket};

/// Player input state sent each tick when input changes.
///
/// Bit flags from Java Input.java:
/// - `FLAG_FORWARD` = 1 (0x01)
/// - `FLAG_BACKWARD` = 2 (0x02)
/// - `FLAG_LEFT` = 4 (0x04)
/// - `FLAG_RIGHT` = 8 (0x08)
/// - `FLAG_JUMP` = 16 (0x10)
/// - `FLAG_SHIFT` = 32 (0x20)
/// - `FLAG_SPRINT` = 64 (0x40)
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPlayerInput {
    pub flags: u8,
}

impl SPlayerInput {
    /// Returns true if the forward key is pressed.
    #[must_use]
    pub const fn forward(&self) -> bool {
        (self.flags & 0x01) != 0
    }

    /// Returns true if the backward key is pressed.
    #[must_use]
    pub const fn backward(&self) -> bool {
        (self.flags & 0x02) != 0
    }

    /// Returns true if the left strafe key is pressed.
    #[must_use]
    pub const fn left(&self) -> bool {
        (self.flags & 0x04) != 0
    }

    /// Returns true if the right strafe key is pressed.
    #[must_use]
    pub const fn right(&self) -> bool {
        (self.flags & 0x08) != 0
    }

    /// Returns true if the jump key is pressed.
    #[must_use]
    pub const fn jump(&self) -> bool {
        (self.flags & 0x10) != 0
    }

    /// Returns true if the shift (sneak) key is pressed.
    #[must_use]
    pub const fn shift(&self) -> bool {
        (self.flags & 0x20) != 0
    }

    /// Returns true if the sprint key is pressed.
    #[must_use]
    pub const fn sprint(&self) -> bool {
        (self.flags & 0x40) != 0
    }
}
