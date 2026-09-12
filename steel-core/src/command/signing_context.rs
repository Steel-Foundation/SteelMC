use std::collections::HashMap;

/// Binary cryptographic signature supplied by the official client (typically 256 bytes for RSA-SHA256).
pub type MessageSignature = Box<[u8]>;

/// Signing metadata and argument signatures associated with an executed command.
#[derive(Clone, Debug)]
pub struct CommandSigningContext {
    /// Client-side emission timestamp in epoch milliseconds.
    pub timestamp: u64,
    /// Random 64-bit salt used to prevent signature replay attacks.
    pub salt: i64,
    /// Map associating each signed Brigadier argument name to its raw binary signature.
    pub argument_signatures: HashMap<Box<str>, MessageSignature>,
}

impl CommandSigningContext {
    /// Creates a new signing context with the given timestamp, salt, and argument signatures.
    pub fn new(
        timestamp: u64,
        salt: i64,
        signatures: impl IntoIterator<Item = (impl Into<Box<str>>, MessageSignature)>,
    ) -> Self {
        Self {
            timestamp,
            salt,
            argument_signatures: signatures.into_iter().map(|(k, v)| (k.into(), v)).collect(),
        }
    }

    /// Returns the raw binary signature for a specific argument name, if present.
    #[must_use]
    pub fn get_argument_signature(&self, argument_name: &str) -> Option<&[u8]> {
        self.argument_signatures.get(argument_name).map(Box::as_ref)
    }

    /// Returns whether this context contains at least one signed argument.
    #[must_use]
    pub fn has_signatures(&self) -> bool {
        !self.argument_signatures.is_empty()
    }
}
