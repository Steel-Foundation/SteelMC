//! Last client input state sent by the player.

use glam::DVec3;

const FLAG_FORWARD: u8 = 0x01;
const FLAG_BACKWARD: u8 = 0x02;
const FLAG_LEFT: u8 = 0x04;
const FLAG_RIGHT: u8 = 0x08;
const FLAG_JUMP: u8 = 0x10;
const FLAG_SHIFT: u8 = 0x20;
const FLAG_SPRINT: u8 = 0x40;

/// Vanilla `Input` snapshot from the latest serverbound player-input packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerInput {
    forward: bool,
    backward: bool,
    left: bool,
    right: bool,
    jump: bool,
    shift: bool,
    sprint: bool,
}

impl PlayerInput {
    /// Empty input state used before the client sends input.
    pub const EMPTY: Self = Self::from_flags(0);

    /// Decodes vanilla `Input` bit flags.
    #[must_use]
    pub const fn from_flags(flags: u8) -> Self {
        Self {
            forward: flags & FLAG_FORWARD != 0,
            backward: flags & FLAG_BACKWARD != 0,
            left: flags & FLAG_LEFT != 0,
            right: flags & FLAG_RIGHT != 0,
            jump: flags & FLAG_JUMP != 0,
            shift: flags & FLAG_SHIFT != 0,
            sprint: flags & FLAG_SPRINT != 0,
        }
    }

    /// Encodes this input snapshot as vanilla `Input` bit flags.
    #[must_use]
    pub const fn flags(self) -> u8 {
        let mut flags = 0;
        flags |= if self.forward { FLAG_FORWARD } else { 0 };
        flags |= if self.backward { FLAG_BACKWARD } else { 0 };
        flags |= if self.left { FLAG_LEFT } else { 0 };
        flags |= if self.right { FLAG_RIGHT } else { 0 };
        flags |= if self.jump { FLAG_JUMP } else { 0 };
        flags |= if self.shift { FLAG_SHIFT } else { 0 };
        flags |= if self.sprint { FLAG_SPRINT } else { 0 };
        flags
    }

    /// Returns true if forward input is pressed.
    #[must_use]
    pub const fn forward(self) -> bool {
        self.forward
    }

    /// Returns true if backward input is pressed.
    #[must_use]
    pub const fn backward(self) -> bool {
        self.backward
    }

    /// Returns true if left input is pressed.
    #[must_use]
    pub const fn left(self) -> bool {
        self.left
    }

    /// Returns true if right input is pressed.
    #[must_use]
    pub const fn right(self) -> bool {
        self.right
    }

    /// Returns true if jump input is pressed.
    #[must_use]
    pub const fn jump(self) -> bool {
        self.jump
    }

    /// Returns true if shift input is pressed.
    #[must_use]
    pub const fn shift(self) -> bool {
        self.shift
    }

    /// Returns true if sprint input is pressed.
    #[must_use]
    pub const fn sprint(self) -> bool {
        self.sprint
    }

    /// Returns vanilla's unrotated left/right movement intent.
    #[must_use]
    pub const fn left_intent(self) -> f32 {
        if self.left == self.right {
            0.0
        } else if self.left {
            1.0
        } else {
            -1.0
        }
    }

    /// Returns vanilla's unrotated forward/backward movement intent.
    #[must_use]
    pub const fn forward_intent(self) -> f32 {
        if self.forward == self.backward {
            0.0
        } else if self.forward {
            1.0
        } else {
            -1.0
        }
    }

    /// Returns the unrotated movement input vector used by vanilla player intent.
    #[must_use]
    pub fn movement_input(self) -> DVec3 {
        DVec3::new(
            f64::from(self.left_intent()),
            0.0,
            f64::from(self.forward_intent()),
        )
    }
}

impl Default for PlayerInput {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[cfg(test)]
mod tests {
    use super::PlayerInput;
    use glam::DVec3;

    #[test]
    fn movement_input_matches_vanilla_intent_axes() {
        assert_eq!(PlayerInput::EMPTY.movement_input(), DVec3::ZERO);
        assert_eq!(
            PlayerInput::from_flags(0x01 | 0x08).movement_input(),
            DVec3::new(-1.0, 0.0, 1.0)
        );
        assert_eq!(
            PlayerInput::from_flags(0x01 | 0x02 | 0x04 | 0x08).movement_input(),
            DVec3::ZERO
        );
    }

    #[test]
    fn input_flags_round_trip() {
        let flags = 0x01 | 0x10 | 0x20 | 0x40;
        let input = PlayerInput::from_flags(flags);

        assert!(input.forward());
        assert!(!input.backward());
        assert!(input.jump());
        assert!(input.shift());
        assert!(input.sprint());
        assert_eq!(input.flags(), flags);
    }
}
