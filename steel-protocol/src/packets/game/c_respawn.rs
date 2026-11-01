//! Clientbound respawn packet - sent to respawn a player or change dimensions.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_RESPAWN;

use super::c_login::CommonPlayerSpawnInfo;

/// Respawn a player in any dimension.
///
/// Sent by the server when a player respawns after death or changes dimensions.
/// The client will reset its world state and prepare for new chunk data.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_RESPAWN)]
pub struct CRespawn {
    /// Dimension and world state the player respawns into.
    pub common_player_spawn_info: CommonPlayerSpawnInfo,
    /// Bit field: 0x01 = keep attribute modifiers, 0x02 = keep entity data.
    /// Set to 0 for a full reset (normal respawn).
    pub data_kept: i8,
}
