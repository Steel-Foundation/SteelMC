use crate::player::LastSeen;
use rustc_hash::FxHashMap;
use steel_protocol::packets::game::MessageSignature;
use uuid::Uuid;

/// Individual signature and sequence index for a signed command argument.
#[derive(Clone, Debug)]
pub struct SignedArgument {
    /// Monotonically increasing index in the player's secure chat session chain.
    pub index: i32,
    /// the original argument before resolving selectors
    pub content: Box<str>,
    /// cryptographic signature absent when the unsigned decoder supplied this argument
    pub signature: Option<MessageSignature>,
}

/// Signing metadata and argument signatures associated with an executed command.
#[derive(Clone, Debug)]
pub struct CommandSigningContext {
    /// Id of the current chat session.
    pub session_id: Uuid,
    /// signed timestamp or unsigned decoding time in epoch milliseconds
    pub timestamp: i64,
    /// Random 64-bit salt used to prevent signature replay attacks.
    pub salt: i64,
    /// decoded messages keyed by their signable Brigadier argument name
    pub argument_signatures: FxHashMap<Box<str>, SignedArgument>,
    /// Window of previously received message signatures acknowledged by the client.
    pub last_seen: LastSeen,
}

impl CommandSigningContext {
    /// Creates a new signing context with the given timestamp, salt, and argument signatures.
    pub fn new(
        session_id: Uuid,
        timestamp: i64,
        salt: i64,
        signatures: impl IntoIterator<Item = (impl Into<Box<str>>, SignedArgument)>,
        last_seen: LastSeen,
    ) -> Self {
        Self {
            session_id,
            timestamp,
            salt,
            argument_signatures: signatures.into_iter().map(|(k, v)| (k.into(), v)).collect(),
            last_seen,
        }
    }

    /// Returns the raw cryptographic signature for a specific argument name, if present.
    #[must_use]
    pub fn get_argument_signature(&self, argument_name: &str) -> Option<&MessageSignature> {
        self.argument_signatures
            .get(argument_name)
            .and_then(|arg| arg.signature.as_ref())
    }

    /// Returns the signed argument metadata (signature and chain index) for a specific argument name, if present.
    #[must_use]
    pub fn get_signed_argument(&self, argument_name: &str) -> Option<&SignedArgument> {
        self.argument_signatures.get(argument_name)
    }

    /// Returns whether this context contains at least one signed argument.
    #[must_use]
    pub fn has_signatures(&self) -> bool {
        self.argument_signatures
            .values()
            .any(|arg| arg.signature.is_some())
    }
}
