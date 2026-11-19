use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};
#[expect(unused_imports)]
use steel_registry::packets::play::S_CHAT_COMMAND;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedRead, ReadFrom, prefixed_read::read_utf};

const MAX_COMMAND_LENGTH: usize = 32_767;
const MAX_ARGUMENT_COUNT: usize = 8;
const MAX_ARGUMENT_NAME_LENGTH: usize = 16;

#[derive(ServerPacket)]
#[packet_id(Play = S_CHAT_COMMAND)]
pub struct SChatCommand {
    pub command: String,
}

impl ReadFrom for SChatCommand {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            command: read_utf(reader, MAX_COMMAND_LENGTH)?,
        })
    }
}

/// Client -> Server: Executes a signed command.
///
/// Commands with signable arguments have each argument individually signed.
/// This prevents tampering with command arguments.
///
/// Equivalent to `ServerboundChatCommandSignedPacket` in Minecraft.
#[derive(ServerPacket, Clone, Debug)]
pub struct SChatCommandSigned {
    /// The command string (without leading slash)
    pub command: String,

    /// Timestamp when command was issued (milliseconds since epoch)
    pub timestamp: i64,

    /// Random salt for uniqueness
    pub salt: i64,

    /// Signatures for each command argument
    pub argument_signatures: Vec<ArgumentSignature>,

    /// Acknowledgment of previously seen messages
    pub last_seen: LastSeenMessagesUpdate,
}

impl ReadFrom for SChatCommandSigned {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            command: read_utf(reader, MAX_COMMAND_LENGTH)?,
            timestamp: i64::read(reader)?,
            salt: i64::read(reader)?,
            argument_signatures: Vec::read_prefixed_bound::<VarInt>(reader, MAX_ARGUMENT_COUNT)?,
            last_seen: LastSeenMessagesUpdate::read(reader)?,
        })
    }
}

/// Signature for a single command argument
#[derive(Clone, Debug)]
pub struct ArgumentSignature {
    /// The argument name
    pub name: String,

    /// The signature bytes (256 bytes for RSA 2048-bit)
    pub signature: [u8; 256],
}

impl ReadFrom for ArgumentSignature {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            name: read_utf(reader, MAX_ARGUMENT_NAME_LENGTH)?,
            signature: <[u8; 256]>::read(reader)?,
        })
    }
}

/// Last seen messages update from client
#[derive(ReadFrom, Clone, Debug)]
pub struct LastSeenMessagesUpdate {
    /// Offset to advance the message window
    pub offset: VarInt,

    /// `BitSet` indicating which of the last 20 messages were acknowledged
    /// 3 bytes = 24 bits (using 20)
    pub acknowledged: [u8; 3],

    /// Checksum of the client's `LastSeenMessages`, verified against the
    /// server's own computed checksum (0 means "skip verification").
    pub checksum: u8,
}
