//! Cryptographic utilities for `SteelMC`, focused on RSA signing and verification
//! for secure chat message validation.
//!
//! This module implements the cryptographic primitives needed for Minecraft's
//! signed chat system, including RSA key pair generation, `SHA256withRSA` signing,
//! and signature verification.
#![expect(
    missing_docs,
    reason = "crypto has a small public surface pending API documentation"
)]
#![expect(
    clippy::absolute_paths,
    clippy::assigning_clones,
    clippy::manual_let_else,
    clippy::missing_panics_doc,
    reason = "crypto code keeps direct error-path tests and small explicit RSA type paths"
)]
#![cfg_attr(
    test,
    expect(
        clippy::unwrap_used,
        reason = "crypto unit tests use unwraps for direct failure diagnostics"
    )
)]

pub mod key_store;
pub mod mojang_api;
pub mod rsa_utils;
pub mod signature;

pub use rsa_utils::{CryptError, generate_key_pair, public_key_from_bytes, public_key_to_bytes};
pub use signature::{SignatureUpdater, SignatureValidator, Signer};

/// Signing algorithm used for chat messages (`SHA256withRSA`)
pub const SIGNING_ALGORITHM: &str = "SHA256withRSA";

/// Size of RSA signatures in bytes (for 1024-bit RSA keys, signatures are 128 bytes)
/// Note: Minecraft protocol specifies 256 bytes, but 1024-bit RSA produces 128-byte signatures
pub const SIGNATURE_BYTES: usize = 128;

/// Size of RSA keys in bits
pub const RSA_KEY_BITS: usize = 1024;
