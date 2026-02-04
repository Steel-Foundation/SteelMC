use std::io::{Cursor, Read};

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedRead, ReadFrom};

/// Client -> Server: Executes a signed command.
///
/// Commands with signable arguments have each argument individually signed.
/// This prevents tampering with command arguments.
///
/// Equivalent to ServerboundChatCommandSignedPacket in Minecraft.
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
        let command = String::read_prefixed_bound::<VarInt>(reader, 256)?;
        let timestamp = i64::read(reader)?;
        let salt = i64::read(reader)?;

        let arg_count = VarInt::read(reader)?.0 as usize;
        if arg_count > 8 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Too many argument signatures",
            ));
        }
        let mut argument_signatures = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            argument_signatures.push(ArgumentSignature::read(reader)?);
        }

        let last_seen = LastSeenMessagesUpdate::read(reader)?;

        Ok(Self {
            command,
            timestamp,
            salt,
            argument_signatures,
            last_seen,
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
        // Read argument name (max 16 chars)
        let name = String::read_prefixed_bound::<VarInt>(reader, 16)?;

        // Read signature
        let mut signature = [0u8; 256];
        reader.read_exact(&mut signature)?;

        Ok(Self { name, signature })
    }
}

/// Last seen messages update from client
#[derive(Clone, Debug)]
pub struct LastSeenMessagesUpdate {
    /// Offset to advance the message window
    pub offset: VarInt,

    /// BitSet indicating which of the last 20 messages were acknowledged
    /// 3 bytes = 24 bits (using 20)
    pub acknowledged: [u8; 3],
}

impl ReadFrom for LastSeenMessagesUpdate {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        let offset = VarInt::read(reader)?;
        let mut acknowledged = [0u8; 3];
        reader.read_exact(&mut acknowledged)?;

        Ok(Self {
            offset,
            acknowledged,
        })
    }
}
