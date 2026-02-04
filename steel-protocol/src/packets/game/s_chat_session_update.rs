use std::io::{Cursor, Read};

use steel_macros::ServerPacket;
use steel_utils::serial::ReadFrom;
use uuid::Uuid;

/// Client -> Server: Updates the player's chat session with their public key.
///
/// Sent when the player first joins or when their key needs to be updated.
/// Contains the session ID and the player's public key signed by Mojang.
///
/// Equivalent to ServerboundChatSessionUpdatePacket in Minecraft.
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

impl ReadFrom for SChatSessionUpdate {
    fn read(reader: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        let session_id = Uuid::read(reader)?;
        let expires_at = i64::read(reader)?;

        let key_len = steel_utils::codec::VarInt::read(reader)?.0 as usize;
        let mut public_key = vec![0u8; key_len];
        reader.read_exact(&mut public_key)?;

        let sig_len = steel_utils::codec::VarInt::read(reader)?.0 as usize;
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
