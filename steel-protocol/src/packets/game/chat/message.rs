use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::codec::VarInt;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SChat {
    #[read(as = Prefixed(VarInt), bound = 256)]
    pub message: String,

    pub timestamp: i64,

    pub salt: i64,

    pub signature: Option<[u8; 256]>,

    #[read(as = VarInt)]
    pub offset: i32,

    pub acknowledged: [u8; 3],

    pub checksum: u8,
}

/// Client -> Server: Acknowledges messages received from the server.
///
/// The client sends this to indicate it has received and processed
/// messages up to the specified offset.
///
/// Equivalent to `ServerboundChatAckPacket` in Minecraft.
#[derive(ServerPacket, Clone, Debug)]
pub struct SChatAck {
    /// The message offset being acknowledged
    pub offset: VarInt,
}

impl steel_utils::serial::ReadFrom for SChatAck {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            offset: VarInt::read(reader)?,
        })
    }
}
