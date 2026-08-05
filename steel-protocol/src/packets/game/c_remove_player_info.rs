//! Packet to remove players from the player list (tab menu).

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_INFO_REMOVE;
use uuid::Uuid;

/// Removes players from the client's player list.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_PLAYER_INFO_REMOVE)]
pub struct CRemovePlayerInfo {
    #[write(as = Prefixed(VarInt))]
    pub uuids: Vec<Uuid>,
}

impl CRemovePlayerInfo {
    /// Creates a packet to remove a single player.
    #[must_use]
    pub fn single(uuid: Uuid) -> Self {
        Self { uuids: vec![uuid] }
    }
}
