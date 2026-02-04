use std::io::Cursor;

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom;

/// Client -> Server: Acknowledges messages received from the server.
///
/// The client sends this to indicate it has received and processed
/// messages up to the specified offset.
///
/// Equivalent to ServerboundChatAckPacket in Minecraft.
#[derive(ServerPacket, Clone, Debug)]
pub struct SChatAck {
    /// The message offset being acknowledged
    pub offset: VarInt,
}

impl ReadFrom for SChatAck {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            offset: VarInt::read(reader)?,
        })
    }
}
