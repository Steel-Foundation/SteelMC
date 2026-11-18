//! Clientbound player combat enter packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_COMBAT_ENTER;

/// Marks that the client's player entered combat.
#[derive(ClientPacket, WriteTo, Clone, Debug, Default)]
#[packet_id(Play = C_PLAYER_COMBAT_ENTER)]
pub struct CPlayerCombatEnter;
