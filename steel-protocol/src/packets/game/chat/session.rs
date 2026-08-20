use std::{
    io::Cursor,
    time::{SystemTime, UNIX_EPOCH},
};

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use steel_utils::serial::PrefixedRead;
use uuid::Uuid;

const MAX_PUBLIC_KEY_SIZE: usize = 512;
const MAX_KEY_SIGNATURE_SIZE: usize = 4096;

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

        let public_key = Vec::<u8>::read_prefixed_bound::<VarInt>(reader, MAX_PUBLIC_KEY_SIZE)?;
        let key_signature =
            Vec::<u8>::read_prefixed_bound::<VarInt>(reader, MAX_KEY_SIGNATURE_SIZE)?;

        Ok(Self {
            session_id,
            expires_at,
            public_key,
            key_signature,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, ErrorKind};

    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use uuid::Uuid;

    use super::{MAX_KEY_SIGNATURE_SIZE, MAX_PUBLIC_KEY_SIZE, SChatSessionUpdate, VarInt};

    fn encoded_packet(
        public_key_length: i32,
        public_key: &[u8],
        signature: Option<(i32, &[u8])>,
    ) -> Vec<u8> {
        let mut packet = Vec::new();
        Uuid::nil()
            .write(&mut packet)
            .expect("test session ID should encode");
        0_i64
            .write(&mut packet)
            .expect("test expiration should encode");
        VarInt(public_key_length)
            .write(&mut packet)
            .expect("test public key length should encode");
        packet.extend_from_slice(public_key);

        if let Some((signature_length, signature)) = signature {
            VarInt(signature_length)
                .write(&mut packet)
                .expect("test signature length should encode");
            packet.extend_from_slice(signature);
        }

        packet
    }

    fn decode(packet: &[u8]) -> std::io::Result<SChatSessionUpdate> {
        SChatSessionUpdate::read(&mut Cursor::new(packet))
    }

    #[test]
    fn accepts_vanilla_maximum_field_lengths() {
        let public_key = vec![1; MAX_PUBLIC_KEY_SIZE];
        let signature = vec![2; MAX_KEY_SIGNATURE_SIZE];
        let packet = encoded_packet(
            MAX_PUBLIC_KEY_SIZE as i32,
            &public_key,
            Some((MAX_KEY_SIGNATURE_SIZE as i32, &signature)),
        );

        let decoded = decode(&packet).expect("fields at the vanilla limits should decode");

        assert_eq!(decoded.public_key, public_key);
        assert_eq!(decoded.key_signature, signature);
    }

    #[test]
    fn rejects_fields_above_vanilla_limits() {
        let oversized_public_key =
            encoded_packet(MAX_PUBLIC_KEY_SIZE as i32 + 1, &[], Some((0, &[])));
        decode(&oversized_public_key).expect_err("an oversized public key should be rejected");

        let oversized_signature =
            encoded_packet(0, &[], Some((MAX_KEY_SIGNATURE_SIZE as i32 + 1, &[])));
        decode(&oversized_signature).expect_err("an oversized signature should be rejected");
    }

    #[test]
    fn rejects_negative_and_extreme_field_lengths() {
        for length in [-1, i32::MAX] {
            let public_key = encoded_packet(length, &[], Some((0, &[])));
            decode(&public_key).expect_err("an invalid public key length should be rejected");

            let signature = encoded_packet(0, &[], Some((length, &[])));
            decode(&signature).expect_err("an invalid signature length should be rejected");
        }
    }

    #[test]
    fn rejects_truncated_field_bodies() {
        let truncated_public_key = encoded_packet(1, &[], None);
        let error = decode(&truncated_public_key)
            .expect_err("a truncated public key body should be rejected");
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);

        let truncated_signature = encoded_packet(0, &[], Some((1, &[])));
        let error = decode(&truncated_signature)
            .expect_err("a truncated signature body should be rejected");
        assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
    }
}
