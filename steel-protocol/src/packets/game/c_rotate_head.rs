//! Packet for updating an entity's head rotation.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ROTATE_HEAD;

/// Updates an entity's head yaw rotation.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ROTATE_HEAD)]
pub struct CRotateHead {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// Head yaw as angle byte
    pub head_y_rot: i8,
}
