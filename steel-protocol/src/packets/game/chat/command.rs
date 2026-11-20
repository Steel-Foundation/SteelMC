use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};
#[expect(unused_imports)]
use steel_registry::packets::play::{S_CHAT_COMMAND, S_CHAT_COMMAND_SIGNED};
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedRead, ReadFrom, prefixed_read::read_utf};

use super::MessageSignature;

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
#[packet_id(Play = S_CHAT_COMMAND_SIGNED)]
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
    pub signature: [u8; MessageSignature::BYTES],
}

impl ReadFrom for ArgumentSignature {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            name: read_utf(reader, MAX_ARGUMENT_NAME_LENGTH)?,
            signature: ReadFrom::read(reader)?,
        })
    }
}

/// Last seen messages update from client
#[derive(ReadFrom, Clone, Debug)]
pub struct LastSeenMessagesUpdate {
    /// Offset to advance the message window
    pub offset: VarInt,

    /// A fixed bitset indicating which msgs in the lastseen window were acked
    pub acknowledged: [u8; LastSeenMessagesUpdate::ACKNOWLEDGED_BYTES],

    /// Checksum of the client's `LastSeenMessages`, verified against the
    /// server's own computed checksum (0 means "skip verification").
    pub checksum: u8,
}

impl LastSeenMessagesUpdate {
    /// the max number of msgs in the lastseen window
    pub const MAX_ENTRIES: usize = 20;
    /// the size of the fixed bitset on the wire
    pub const ACKNOWLEDGED_BYTES: usize = Self::MAX_ENTRIES.div_ceil(u8::BITS as usize);
}
