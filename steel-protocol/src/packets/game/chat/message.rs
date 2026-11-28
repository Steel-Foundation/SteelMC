use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::codec::VarInt;
use steel_utils::serial::{ReadFrom, prefixed_read::read_utf};

use super::{LastSeenMessagesUpdate, MessageSignature};

const MAX_CHAT_LENGTH: usize = 256;

#[derive(ServerPacket, Clone, Debug)]
pub struct SChat {
    pub message: String,
    pub timestamp: i64,
    pub salt: i64,
    pub signature: Option<[u8; MessageSignature::BYTES]>,
    pub last_seen: LastSeenMessagesUpdate,
}

impl ReadFrom for SChat {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            message: read_utf(reader, MAX_CHAT_LENGTH)?,
            timestamp: i64::read(reader)?,
            salt: i64::read(reader)?,
            signature: Option::read(reader)?,
            last_seen: LastSeenMessagesUpdate::read(reader)?,
        })
    }
}

/// Client -> Server: Acknowledges messages received from the server.
///
/// The client sends this to indicate it has received and processed
/// messages up to the specified offset.
///
/// Equivalent to `ServerboundChatAckPacket` in Minecraft.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SChatAck {
    /// The message offset being acknowledged
    pub offset: VarInt,
}
