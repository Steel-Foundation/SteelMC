//! This module contains the `KeyStore` struct, which is used to store the server's encryption keys.
use rsa::{RsaPrivateKey, RsaPublicKey};

/// A struct that stores the server's encryption keys.
pub struct KeyStore {
    /// The server's private key.
    pub private_key: RsaPrivateKey,
    /// The server's public key in DER format.
    pub public_key_der: Vec<u8>,
}

impl KeyStore {
    /// Creates a new `KeyStore`.
    #[must_use]
    pub fn create() -> Self {
        log::debug!("Creating encryption keys...");
        let private_key = Self::generate_private_key();

        let public_key = RsaPublicKey::from(&private_key);
        let public_key_der =
            crate::public_key_to_bytes(&public_key).expect("Failed to encode public key");

        Self {
            private_key,
            public_key_der,
        }
    }

    fn generate_private_key() -> RsaPrivateKey {
        // Found out that OsRng is faster than rand::thread_rng here
        let mut rng = rand::rng();

        RsaPrivateKey::new(&mut rng, 1024).expect("Failed to generate a key")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_der_round_trips() {
        let ks = KeyStore::create();
        let decoded = crate::public_key_from_bytes(&ks.public_key_der).unwrap();
        assert_eq!(decoded, RsaPublicKey::from(&ks.private_key));
    }
}
