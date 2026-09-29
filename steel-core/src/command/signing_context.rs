use crate::player::LastSeen;
use rustc_hash::FxHashMap;
use steel_protocol::packets::game::MessageSignature;

/// Signing metadata and argument signatures associated with an executed command.
#[derive(Clone, Debug)]
pub struct CommandSigningContext {
    /// Client-side emission timestamp in epoch milliseconds.
    pub timestamp: u64,
    /// Random 64-bit salt used to prevent signature replay attacks.
    pub salt: i64,
    /// Map associating each signed Brigadier argument name with its raw binary signature.
    pub argument_signatures: FxHashMap<Box<str>, MessageSignature>,
    /// Window of previously received message signatures acknowledged by the client when submitting this command.
    pub last_seen: LastSeen,
    /// Monotonically increasing the index of this message within the player's secure chat session chain.
    pub sender_index: i32,
}

impl CommandSigningContext {
    /// Creates a new signing context with the given timestamp, salt, and argument signatures.
    pub fn new(
        timestamp: u64,
        salt: i64,
        signatures: impl IntoIterator<Item = (impl Into<Box<str>>, MessageSignature)>,
        last_seen: LastSeen,
        sender_index: i32,
    ) -> Self {
        Self {
            timestamp,
            salt,
            argument_signatures: signatures.into_iter().map(|(k, v)| (k.into(), v)).collect(),
            last_seen,
            sender_index,
        }
    }

    /// Returns the raw binary signature for a specific argument name, if present.
    #[must_use]
    pub fn get_argument_signature(&self, argument_name: &str) -> Option<&MessageSignature> {
        self.argument_signatures.get(argument_name)
    }

    /// Returns whether this context contains at least one signed argument.
    #[must_use]
    pub fn has_signatures(&self) -> bool {
        !self.argument_signatures.is_empty()
    }
}
