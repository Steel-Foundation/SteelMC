//! RSA signature creation and verification utilities.
//!
//! Ported from net/minecraft/util/Signer.java and SignatureValidator.java

use rsa::pkcs1v15::SigningKey;
use rsa::sha2::Sha256;
use rsa::signature::{SignatureEncoding, Signer as RsaSigner, Verifier};
use rsa::{RsaPrivateKey, RsaPublicKey};

use crate::rsa_utils::CryptError;

/// A function that updates signature data by writing bytes.
///
/// Equivalent to `SignatureUpdater` in Minecraft.
pub trait SignatureUpdater {
    /// Updates the signature with the given bytes.
    fn update(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError>;
}

/// Output receiver for signature data.
///
/// Equivalent to SignatureUpdater.Output in Minecraft.
pub trait SignatureOutput {
    /// Receives bytes to be added to the signature.
    fn update(&mut self, data: &[u8]) -> Result<(), CryptError>;
}

/// Implementation of `SignatureUpdater` for raw byte slices.
impl SignatureUpdater for &[u8] {
    fn update(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
        output.update(self)
    }
}

/// A trait for signing data with an RSA private key.
///
/// Equivalent to Signer interface in Minecraft.
pub trait Signer {
    /// Signs the data provided by the updater and returns the signature bytes.
    fn sign(&self, updater: &dyn SignatureUpdater) -> Result<Vec<u8>, CryptError>;
}

/// A trait for validating RSA signatures.
///
/// Equivalent to `SignatureValidator` interface in Minecraft.
pub trait SignatureValidator {
    /// Validates that the signature is valid for the data provided by the updater.
    fn validate(
        &self,
        updater: &dyn SignatureUpdater,
        signature: &[u8],
    ) -> Result<bool, CryptError>;
}

/// Creates a signer from an RSA private key using `SHA256withRSA`.
///
/// Equivalent to Signer.from(PrivateKey, "`SHA256withRSA`") in Minecraft.
pub struct RsaPrivateKeySigner {
    signing_key: SigningKey<Sha256>,
}

impl RsaPrivateKeySigner {
    #[must_use]
    pub fn new(private_key: RsaPrivateKey) -> Self {
        Self {
            signing_key: SigningKey::new(private_key),
        }
    }
}

impl Signer for RsaPrivateKeySigner {
    fn sign(&self, updater: &dyn SignatureUpdater) -> Result<Vec<u8>, CryptError> {
        // Collect all bytes to sign
        let mut collector = ByteCollector::new();
        updater.update(&mut collector)?;

        // Sign the collected data
        let signature = self.signing_key.sign(&collector.bytes);
        Ok(signature.to_bytes().as_ref().to_vec())
    }
}

/// Creates a signature validator from an RSA public key using `SHA256withRSA`.
///
/// Equivalent to SignatureValidator.from(PublicKey, "`SHA256withRSA`") in Minecraft.
pub struct RsaPublicKeyValidator {
    verifying_key: rsa::pkcs1v15::VerifyingKey<Sha256>,
}

impl RsaPublicKeyValidator {
    #[must_use]
    pub fn new(public_key: RsaPublicKey) -> Self {
        Self {
            verifying_key: rsa::pkcs1v15::VerifyingKey::new(public_key),
        }
    }
}

impl SignatureValidator for RsaPublicKeyValidator {
    fn validate(
        &self,
        updater: &dyn SignatureUpdater,
        signature_bytes: &[u8],
    ) -> Result<bool, CryptError> {
        // Collect all bytes to verify
        let mut collector = ByteCollector::new();
        updater.update(&mut collector)?;

        // Parse signature
        let signature = match rsa::pkcs1v15::Signature::try_from(signature_bytes) {
            Ok(sig) => sig,
            Err(_) => return Ok(false),
        };

        // Verify the signature
        Ok(self
            .verifying_key
            .verify(&collector.bytes, &signature)
            .is_ok())
    }
}

/// A multi-key validator that tries each public key until one validates.
///
/// Used for Mojang's multiple player certificate keys.
pub struct MultiKeyValidator {
    validators: Vec<RsaPublicKeyValidator>,
}

impl MultiKeyValidator {
    pub fn new(public_keys: Vec<RsaPublicKey>) -> Self {
        Self {
            validators: public_keys
                .into_iter()
                .map(RsaPublicKeyValidator::new)
                .collect(),
        }
    }
}

impl SignatureValidator for MultiKeyValidator {
    fn validate(
        &self,
        updater: &dyn SignatureUpdater,
        signature: &[u8],
    ) -> Result<bool, CryptError> {
        // Try each validator - if any succeeds, the signature is valid
        for validator in &self.validators {
            if validator.validate(updater, signature)? {
                return Ok(true);
            }
        }
        // None of the keys validated the signature
        Ok(false)
    }
}

/// A no-validation validator that always returns true.
///
/// Equivalent to `SignatureValidator.NO_VALIDATION` in Minecraft.
pub struct NoValidation;

impl SignatureValidator for NoValidation {
    fn validate(
        &self,
        _updater: &dyn SignatureUpdater,
        _signature: &[u8],
    ) -> Result<bool, CryptError> {
        Ok(true)
    }
}

/// Helper struct to collect bytes during signature operations.
struct ByteCollector {
    bytes: Vec<u8>,
}

impl ByteCollector {
    const fn new() -> Self {
        Self { bytes: Vec::new() }
    }
}

impl SignatureOutput for ByteCollector {
    fn update(&mut self, data: &[u8]) -> Result<(), CryptError> {
        self.bytes.extend_from_slice(data);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rsa_utils::generate_key_pair;

    struct TestUpdater {
        data: Vec<u8>,
    }

    impl SignatureUpdater for TestUpdater {
        fn update(&self, output: &mut dyn SignatureOutput) -> Result<(), CryptError> {
            output.update(&self.data)
        }
    }

    #[test]
    fn test_sign_and_verify() {
        let (private_key, public_key) = generate_key_pair().unwrap();

        let signer = RsaPrivateKeySigner::new(private_key);
        let validator = RsaPublicKeyValidator::new(public_key);

        let data = b"Hello, signed chat!";
        let updater = TestUpdater {
            data: data.to_vec(),
        };

        let signature = signer.sign(&updater).unwrap();
        // RSA 1024-bit produces 128-byte signatures, not 256
        // Minecraft uses 256 bytes but that's for padding or 2048-bit keys
        assert_eq!(signature.len(), 128);

        let is_valid = validator.validate(&updater, &signature).unwrap();
        assert!(is_valid);
    }

    #[test]
    fn test_invalid_signature() {
        let (_, public_key) = generate_key_pair().unwrap();
        let validator = RsaPublicKeyValidator::new(public_key);

        let data = b"Hello, signed chat!";
        let updater = TestUpdater {
            data: data.to_vec(),
        };

        let bad_signature = vec![0u8; crate::SIGNATURE_BYTES];
        let is_valid = validator.validate(&updater, &bad_signature).unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_no_validation() {
        let validator = NoValidation;
        let data = b"Any data";
        let updater = TestUpdater {
            data: data.to_vec(),
        };
        let signature = vec![0u8; 10];

        let is_valid = validator.validate(&updater, &signature).unwrap();
        assert!(is_valid);
    }
}
