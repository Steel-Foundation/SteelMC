//! Clientbound player combat end packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_COMBAT_END;

/// Marks that the client's player left combat, carrying the combat duration.
#[derive(ClientPacket, WriteTo, Clone, Debug, Default)]
#[packet_id(Play = C_PLAYER_COMBAT_END)]
pub struct CPlayerCombatEnd {
    /// Duration of the combat in ticks.
    #[write(as = VarInt)]
    pub duration: i32,
}
