//! Player profile public key management for secure chat.
//!
//! Ported from net/minecraft/world/entity/player/ProfilePublicKey.java

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rsa::RsaPublicKey;
use steel_crypto::{
    CryptError, SignatureValidator, public_key_from_bytes, public_key_to_bytes,
    signature::RsaPublicKeyValidator,
};
use steel_protocol::packets::game::ProtocolRemoteChatSessionData;
use thiserror::Error;
use uuid::Uuid;

/// Grace period for expired keys (8 hours, as in vanilla)
pub const EXPIRY_GRACE_PERIOD: Duration = Duration::from_hours(8);

/// Maximum size of key signature in bytes
pub const MAX_KEY_SIGNATURE_SIZE: usize = 4096;

/// Errors that can occur during profile key validation
#[derive(Debug, Error)]
pub enum ValidationError {
    /// Invalid public key signature
    #[error("Invalid public key signature")]
    InvalidSignature,

    /// Key has expired
    #[error("Key has expired")]
    KeyExpired,

    /// Cryptographic error
    #[error("Cryptographic error: {0}")]
    CryptoError(#[from] CryptError),
}

/// Profile public key data containing key, expiry, and Mojang signature.
///
/// Equivalent to ProfilePublicKey.Data in Minecraft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfilePublicKeyData {
    /// When this key expires
    pub expires_at: SystemTime,

    /// The RSA public key
    pub key: RsaPublicKey,

    /// Signature of this key signed by Mojang's services
    pub key_signature: Vec<u8>,
}

impl ProfilePublicKeyData {
    /// Creates new profile public key data
    #[must_use]
    pub const fn new(expires_at: SystemTime, key: RsaPublicKey, key_signature: Vec<u8>) -> Self {
        Self {
            expires_at,
            key,
            key_signature,
        }
    }

    /// Checks if the key has expired (without grace period)
    #[must_use]
    pub fn has_expired(&self) -> bool {
        // Time has passed since expiry
        self.expires_at.elapsed().is_ok()
    }

    /// Checks if the key has expired, with a grace period
    #[must_use]
    pub fn has_expired_with_grace(&self, grace_period: Duration) -> bool {
        let expiry_with_grace = self.expires_at + grace_period;
        expiry_with_grace.elapsed().is_ok()
    }

    /// Validates the key signature using Mojang's signature validator.
    ///
    /// The signature covers: profileId + expiresAt + key bytes
    ///
    /// # Errors
    /// Returns `ValidationError` if signature validation fails or cryptographic operations fail
    pub fn validate_signature(
        &self,
        profile_id: Uuid,
        validator: &dyn SignatureValidator,
    ) -> Result<(), ValidationError> {
        let payload = self.signed_payload(profile_id)?;
        let updater = ByteSliceUpdater(&payload);

        let is_valid = validator
            .validate(&updater, &self.key_signature)
            .map_err(ValidationError::from)?;

        if is_valid {
            Ok(())
        } else {
            Err(ValidationError::InvalidSignature)
        }
    }

    /// Constructs the byte payload that was signed by Mojang.
    ///
    /// Format: profileId (16 bytes) + expiresAt (8 bytes) + key bytes
    fn signed_payload(&self, profile_id: Uuid) -> Result<Vec<u8>, ValidationError> {
        let key_bytes = public_key_to_bytes(&self.key)?;

        let mut payload = Vec::with_capacity(24 + key_bytes.len());

        // Profile UUID (most significant bits + least significant bits, big-endian)
        payload.extend_from_slice(&profile_id.as_u128().to_be_bytes());

        // Expiry timestamp (milliseconds since epoch, big-endian)
        let expiry_millis = system_time_to_millis(self.expires_at);
        payload.extend_from_slice(&expiry_millis.to_be_bytes());

        // Public key bytes
        payload.extend_from_slice(&key_bytes);

        Ok(payload)
    }

    /// Serializes the key data for network transmission
    ///
    /// # Errors
    /// Returns `ValidationError` if key encoding fails
    pub fn to_bytes(&self) -> Result<Vec<u8>, ValidationError> {
        let mut bytes = Vec::new();

        // Expiry timestamp (i64 milliseconds)
        let expiry_millis = system_time_to_millis(self.expires_at);
        bytes.extend_from_slice(&expiry_millis.to_be_bytes());

        // Public key
        let key_bytes = public_key_to_bytes(&self.key)?;
        bytes.extend_from_slice(&(key_bytes.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&key_bytes);

        // Key signature
        bytes.extend_from_slice(&(self.key_signature.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&self.key_signature);

        Ok(bytes)
    }

    /// Deserializes key data from bytes
    ///
    /// # Errors
    /// Returns `ValidationError` if the byte format is invalid or key decoding fails
    ///
    /// # Panics
    /// Panics if slice-to-array conversion fails (should not happen due to length checks)
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ValidationError> {
        if bytes.len() < 16 {
            return Err(ValidationError::CryptoError(CryptError::InvalidKeyFormat));
        }

        let mut offset = 0;

        // Read expiry timestamp
        let expiry_millis = i64::from_be_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .expect("slice is exactly 8 bytes"),
        );
        offset += 8;
        let expires_at = system_time_from_millis(expiry_millis);

        // Read public key length
        if bytes.len() < offset + 4 {
            return Err(ValidationError::CryptoError(CryptError::InvalidKeyFormat));
        }
        let key_len = i32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        ) as usize;
        offset += 4;

        // Read public key
        if bytes.len() < offset + key_len {
            return Err(ValidationError::CryptoError(CryptError::InvalidKeyFormat));
        }
        let key = public_key_from_bytes(&bytes[offset..offset + key_len])?;
        offset += key_len;

        // Read signature length
        if bytes.len() < offset + 4 {
            return Err(ValidationError::CryptoError(CryptError::InvalidKeyFormat));
        }
        let sig_len = i32::from_be_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("slice is exactly 4 bytes"),
        ) as usize;
        offset += 4;

        // Read signature
        if bytes.len() < offset + sig_len {
            return Err(ValidationError::CryptoError(CryptError::InvalidKeyFormat));
        }
        let key_signature = bytes[offset..offset + sig_len].to_vec();

        Ok(Self {
            expires_at,
            key,
            key_signature,
        })
    }
}

/// Validated profile public key.
///
/// Equivalent to `ProfilePublicKey` in Minecraft.
#[derive(Clone, Debug)]
pub struct ProfilePublicKey {
    data: ProfilePublicKeyData,
}

impl ProfilePublicKey {
    /// Creates a new validated profile public key.
    ///
    /// This should only be called after validating the signature.
    #[must_use]
    pub const fn new(data: ProfilePublicKeyData) -> Self {
        Self { data }
    }

    /// Validates and creates a profile public key.
    ///
    /// Equivalent to `ProfilePublicKey.createValidated()` in Minecraft.
    ///
    /// # Errors
    /// Returns `ValidationError` if signature validation fails
    pub fn create_validated(
        profile_id: Uuid,
        data: ProfilePublicKeyData,
        validator: &dyn SignatureValidator,
    ) -> Result<Self, ValidationError> {
        data.validate_signature(profile_id, validator)?;
        Ok(Self::new(data))
    }

    /// Gets the underlying key data
    #[must_use]
    pub const fn data(&self) -> &ProfilePublicKeyData {
        &self.data
    }

    /// Creates a signature validator for this key
    #[must_use]
    pub fn create_signature_validator(&self) -> RsaPublicKeyValidator {
        RsaPublicKeyValidator::new(self.data.key.clone())
    }
}

/// Remote chat session containing session ID and validated public key.
///
/// Equivalent to `RemoteChatSession` in Minecraft.
#[derive(Clone, Debug)]
pub struct RemoteChatSession {
    /// The session ID
    pub session_id: Uuid,
    /// The validated profile public key
    pub profile_public_key: ProfilePublicKey,
}

impl RemoteChatSession {
    /// Creates a new remote chat session
    #[must_use]
    pub const fn new(session_id: Uuid, profile_public_key: ProfilePublicKey) -> Self {
        Self {
            session_id,
            profile_public_key,
        }
    }

    /// Checks if the key has expired
    #[must_use]
    pub fn has_expired(&self) -> bool {
        self.profile_public_key.data().has_expired()
    }

    /// Converts to data for network transmission
    #[must_use]
    pub fn as_data(&self) -> RemoteChatSessionData {
        RemoteChatSessionData {
            session_id: self.session_id,
            profile_public_key: self.profile_public_key.data().clone(),
        }
    }
}

/// Network-serializable chat session data.
///
/// Equivalent to `RemoteChatSession.Data` in Minecraft.
#[derive(Clone, Debug)]
pub struct RemoteChatSessionData {
    /// The session ID
    pub session_id: Uuid,
    /// The profile public key data
    pub profile_public_key: ProfilePublicKeyData,
}

impl RemoteChatSessionData {
    /// Validates and creates a `RemoteChatSession`
    ///
    /// # Errors
    /// Returns `ValidationError` if signature validation fails
    pub fn validate(
        self,
        profile_id: Uuid,
        validator: &dyn SignatureValidator,
    ) -> Result<RemoteChatSession, ValidationError> {
        let public_key =
            ProfilePublicKey::create_validated(profile_id, self.profile_public_key, validator)?;
        Ok(RemoteChatSession::new(self.session_id, public_key))
    }

    /// Converts to network-serializable format for transmission
    ///
    /// # Errors
    /// Returns `ValidationError` if key encoding fails
    pub fn to_protocol_data(&self) -> Result<ProtocolRemoteChatSessionData, ValidationError> {
        let key_bytes = public_key_to_bytes(&self.profile_public_key.key)?;

        Ok(ProtocolRemoteChatSessionData {
            session_id: self.session_id,
            expires_at_millis: system_time_to_millis(self.profile_public_key.expires_at),
            public_key_bytes: key_bytes,
            key_signature: self.profile_public_key.key_signature.clone(),
        })
    }
}

pub(super) fn system_time_from_millis(millis: i64) -> SystemTime {
    if millis >= 0 {
        UNIX_EPOCH + Duration::from_millis(millis as u64)
    } else {
        UNIX_EPOCH - Duration::from_millis(millis.unsigned_abs())
    }
}

fn system_time_to_millis(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_millis()).unwrap_or(i64::MAX),
        Err(error) => match i64::try_from(error.duration().as_millis()) {
            Ok(millis) => millis.saturating_neg(),
            Err(_) => i64::MIN,
        },
    }
}

// Helper struct for byte slice updater
mod signature_helpers {
    use steel_crypto::{
        CryptError,
        signature::{SignatureOutput, SignatureUpdater},
    };

    pub struct ByteSliceUpdater<'a>(pub &'a [u8]);

    impl SignatureUpdater for ByteSliceUpdater<'_> {
        fn update(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
            output.update(self.0)
        }
    }
}

use signature_helpers::ByteSliceUpdater;

#[cfg(test)]
mod tests {
    use rsa::{
        pkcs1v15::SigningKey,
        signature::{SignatureEncoding as _, Signer as _},
    };
    use sha1::Sha1;
    use steel_crypto::{generate_key_pair, public_key_to_bytes, signature::ProfileKeyValidator};
    use uuid::Uuid;

    use super::{ProfilePublicKeyData, ValidationError, system_time_from_millis};

    fn signed_profile_key(
        profile_id: Uuid,
        expires_at_millis: i64,
    ) -> (ProfilePublicKeyData, ProfileKeyValidator) {
        let (service_private_key, service_public_key) =
            generate_key_pair().expect("test service key should generate");
        let (_, player_public_key) = generate_key_pair().expect("test player key should generate");
        let player_key_der =
            public_key_to_bytes(&player_public_key).expect("test player key should encode");
        let mut payload = Vec::with_capacity(24 + player_key_der.len());
        payload.extend_from_slice(&profile_id.as_u128().to_be_bytes());
        payload.extend_from_slice(&expires_at_millis.to_be_bytes());
        payload.extend_from_slice(&player_key_der);
        let signature = SigningKey::<Sha1>::new(service_private_key)
            .sign(&payload)
            .to_bytes()
            .to_vec();

        (
            ProfilePublicKeyData::new(
                system_time_from_millis(expires_at_millis),
                player_public_key,
                signature,
            ),
            ProfileKeyValidator::new(vec![service_public_key])
                .expect("test service key should create a validator"),
        )
    }

    #[test]
    fn profile_key_signature_binds_uuid_expiry_and_public_key() {
        let profile_id = Uuid::from_u128(1);
        let (data, validator) = signed_profile_key(profile_id, 1_234);

        data.validate_signature(profile_id, &validator)
            .expect("untampered profile key should validate");
        assert!(matches!(
            data.validate_signature(Uuid::from_u128(2), &validator),
            Err(ValidationError::InvalidSignature)
        ));

        let mut changed_expiry = data.clone();
        changed_expiry.expires_at = system_time_from_millis(1_235);
        assert!(matches!(
            changed_expiry.validate_signature(profile_id, &validator),
            Err(ValidationError::InvalidSignature)
        ));

        let mut changed_key = data.clone();
        changed_key.key = generate_key_pair()
            .expect("replacement player key should generate")
            .1;
        assert!(matches!(
            changed_key.validate_signature(profile_id, &validator),
            Err(ValidationError::InvalidSignature)
        ));
    }

    #[test]
    fn profile_key_signature_rejects_tampered_signature() {
        let profile_id = Uuid::from_u128(1);
        let (mut data, validator) = signed_profile_key(profile_id, 1_234);
        data.key_signature[0] ^= 1;

        assert!(matches!(
            data.validate_signature(profile_id, &validator),
            Err(ValidationError::InvalidSignature)
        ));
    }

    #[test]
    fn signed_payload_preserves_negative_expiry_millis() {
        let profile_id = Uuid::from_u128(1);
        let (data, validator) = signed_profile_key(profile_id, -1);

        data.validate_signature(profile_id, &validator)
            .expect("pre-epoch expiry should retain its signed timestamp");
    }
}
