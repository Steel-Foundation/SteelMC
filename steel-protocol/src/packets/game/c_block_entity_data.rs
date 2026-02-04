//! Block entity data packet.
//!
//! Sent by the server to update block entity data on the client.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_BLOCK_ENTITY_DATA;
use steel_utils::BlockPos;
use steel_utils::serial::OptionalNbt;

/// Packet sent to update a block entity's data on the client.
///
/// Used to synchronize block entity data such as sign text, banner patterns, etc.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_BLOCK_ENTITY_DATA)]
pub struct CBlockEntityData {
    /// Position of the block entity.
    pub pos: BlockPos,
    /// Type ID of the block entity (from the block entity type registry).
    #[write(as = VarInt)]
    pub block_entity_type: i32,
    /// NBT data for the block entity (uses OptionalNbt format with tag type prefix).
    pub nbt: OptionalNbt,
}
