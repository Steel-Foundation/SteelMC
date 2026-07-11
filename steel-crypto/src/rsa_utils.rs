//! RSA key pair generation and encoding utilities.
//!
//! Ported from net/minecraft/util/Crypt.java

use rsa::{RsaPrivateKey, RsaPublicKey};
use thiserror::Error;

/// Errors that can occur during cryptographic operations
#[derive(Debug, Error)]
pub enum CryptError {
    #[error("RSA error: {0}")]
    RsaError(#[from] rsa::Error),

    #[error("PKCS8 error: {0}")]
    Pkcs8Error(#[from] rsa::pkcs8::Error),

    #[error("SPKI error: {0}")]
    SpkiError(#[from] rsa::pkcs8::spki::Error),

    #[error("Invalid key format")]
    InvalidKeyFormat,

    #[error("Cryptographic operation failed: {0}")]
    OperationFailed(String),
}

/// Generates a 1024-bit RSA key pair.
///
/// Equivalent to `Crypt.generateKeyPair()` in Minecraft.
pub fn generate_key_pair() -> Result<(RsaPrivateKey, RsaPublicKey), CryptError> {
    let mut rng = rand::rng();
    let private_key = RsaPrivateKey::new(&mut rng, crate::RSA_KEY_BITS)?;
    let public_key = RsaPublicKey::from(&private_key);
    Ok((private_key, public_key))
}

/// Converts an RSA public key to DER-encoded X.509 format bytes.
///
/// This is the format sent over the network.
pub fn public_key_to_bytes(key: &RsaPublicKey) -> Result<Vec<u8>, CryptError> {
    use rsa::pkcs8::EncodePublicKey;
    key.to_public_key_der()
        .map(|der| der.to_vec())
        .map_err(CryptError::from)
}

/// Parses an RSA public key from DER-encoded X.509 format bytes.
///
/// Equivalent to `Crypt.byteToPublicKey()` in Minecraft.
pub fn public_key_from_bytes(bytes: &[u8]) -> Result<RsaPublicKey, CryptError> {
    use rsa::pkcs8::DecodePublicKey;
    RsaPublicKey::from_public_key_der(bytes).map_err(CryptError::from)
}

/// Converts an RSA private key to PKCS8 DER format bytes.
pub fn private_key_to_bytes(key: &RsaPrivateKey) -> Result<Vec<u8>, CryptError> {
    use rsa::pkcs8::EncodePrivateKey;
    key.to_pkcs8_der()
        .map(|der| der.to_bytes().to_vec())
        .map_err(CryptError::from)
}

/// Parses an RSA private key from PKCS8 DER format bytes.
///
/// Equivalent to `Crypt.byteToPrivateKey()` in Minecraft.
pub fn private_key_from_bytes(bytes: &[u8]) -> Result<RsaPrivateKey, CryptError> {
    use rsa::pkcs8::DecodePrivateKey;
    RsaPrivateKey::from_pkcs8_der(bytes).map_err(CryptError::from)
}

/// Converts an RSA public key to PEM format string.
///
/// Format: "-----BEGIN RSA PUBLIC KEY-----\n{base64}\n-----END RSA PUBLIC KEY-----\n"
pub fn public_key_to_pem(key: &RsaPublicKey) -> Result<String, CryptError> {
    use rsa::pkcs8::EncodePublicKey;
    key.to_public_key_pem(rsa::pkcs8::LineEnding::LF)
        .map_err(CryptError::from)
}

/// Parses an RSA public key from PEM format string.
///
/// Equivalent to `Crypt.stringToRsaPublicKey()` in Minecraft.
pub fn public_key_from_pem(pem: &str) -> Result<RsaPublicKey, CryptError> {
    use rsa::pkcs8::DecodePublicKey;
    RsaPublicKey::from_public_key_pem(pem).map_err(CryptError::from)
}

/// Converts an RSA private key to PEM format string.
pub fn private_key_to_pem(key: &RsaPrivateKey) -> Result<String, CryptError> {
    use rsa::pkcs8::EncodePrivateKey;
    key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
        .map(|pem| pem.to_string())
        .map_err(CryptError::from)
}

/// Parses an RSA private key from PEM format string.
///
/// Equivalent to `Crypt.stringToPemRsaPrivateKey()` in Minecraft.
pub fn private_key_from_pem(pem: &str) -> Result<RsaPrivateKey, CryptError> {
    use rsa::pkcs8::DecodePrivateKey;
    RsaPrivateKey::from_pkcs8_pem(pem).map_err(CryptError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_key_pair() {
        let result = generate_key_pair();
        assert!(result.is_ok());
    }

    #[test]
    fn test_public_key_round_trip() {
        let (_, public_key) = generate_key_pair().unwrap();
        let bytes = public_key_to_bytes(&public_key).unwrap();
        let decoded = public_key_from_bytes(&bytes).unwrap();

        let encoded_again = public_key_to_bytes(&decoded).unwrap();
        assert_eq!(bytes, encoded_again);
    }

    #[test]
    fn test_pem_round_trip() {
        let (_, public_key) = generate_key_pair().unwrap();
        let pem = public_key_to_pem(&public_key).unwrap();
        let decoded = public_key_from_pem(&pem).unwrap();

        let pem_again = public_key_to_pem(&decoded).unwrap();
        assert_eq!(pem, pem_again);
    }
}
