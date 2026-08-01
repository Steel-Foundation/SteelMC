use std::{
    io::{Cursor, Read},
    time::{SystemTime, UNIX_EPOCH},
};

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use uuid::Uuid;

/// Network-serializable chat session data.
///
/// This is a simplified version that holds raw byte data for transmission.
/// The full version with validated keys lives in steel-core.
#[derive(Clone, Debug)]
pub struct ProtocolRemoteChatSessionData {
    /// The session ID
    pub session_id: Uuid,
    /// When the key expires (as milliseconds since UNIX epoch)
    pub expires_at_millis: i64,
    /// The public key bytes
    pub public_key_bytes: Vec<u8>,
    /// The key signature bytes
    pub key_signature: Vec<u8>,
}

impl ProtocolRemoteChatSessionData {
    /// Creates new chat session data from raw components
    #[must_use]
    pub fn new(
        session_id: Uuid,
        expires_at: SystemTime,
        public_key_bytes: Vec<u8>,
        key_signature: Vec<u8>,
    ) -> Self {
        let expires_at_millis = expires_at
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        Self {
            session_id,
            expires_at_millis,
            public_key_bytes,
            key_signature,
        }
    }
}

impl steel_utils::serial::WriteTo for ProtocolRemoteChatSessionData {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.session_id.write(writer)?;

        // Write expires_at (i64 millis)
        self.expires_at_millis.write(writer)?;

        // Write public key (length as VarInt, then bytes)
        VarInt(self.public_key_bytes.len() as i32).write(writer)?;
        writer.write_all(&self.public_key_bytes)?;

        // Write key signature (length as VarInt, then bytes)
        VarInt(self.key_signature.len() as i32).write(writer)?;
        writer.write_all(&self.key_signature)?;

        Ok(())
    }
}

/// Client -> Server: Updates the player's chat session with their public key.
///
/// Sent when the player first joins or when their key needs to be updated.
/// Contains the session ID and the player's public key signed by Mojang.
///
/// Equivalent to `ServerboundChatSessionUpdatePacket` in Minecraft.
#[derive(ServerPacket, Clone, Debug)]
pub struct SChatSessionUpdate {
    /// The session ID for this chat session
    pub session_id: Uuid,

    /// Public key expiry timestamp (milliseconds since epoch)
    pub expires_at: i64,

    /// The player's RSA public key (DER encoded)
    pub public_key: Vec<u8>,

    /// Mojang's signature of the key (validates authenticity)
    pub key_signature: Vec<u8>,
}

impl steel_utils::serial::ReadFrom for SChatSessionUpdate {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        let session_id = Uuid::read(reader)?;
        let expires_at = i64::read(reader)?;

        let key_len = VarInt::read(reader)?.0 as usize;
        let mut public_key = vec![0u8; key_len];
        reader.read_exact(&mut public_key)?;

        let sig_len = VarInt::read(reader)?.0 as usize;
        let mut key_signature = vec![0u8; sig_len];
        reader.read_exact(&mut key_signature)?;

        Ok(Self {
            session_id,
            expires_at,
            public_key,
            key_signature,
        })
    }
}
