use steel_macros::{ReadFrom, ServerPacket};

/// Sent by the client when the player starts or stops flying.
/// The server uses this to track the player's flying state.
#[derive(ServerPacket, ReadFrom, Clone, Debug)]
pub struct SPlayerAbilities {
    /// Bitfield containing only the FLYING flag (0x02 if flying, 0x00 if not)
    pub flags: u8,
}

impl SPlayerAbilities {
    const FLAG_FLYING: u8 = 0x02;

    /// Returns whether the player is flying
    pub fn is_flying(&self) -> bool {
        (self.flags & Self::FLAG_FLYING) != 0
    }
}
