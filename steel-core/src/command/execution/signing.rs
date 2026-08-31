//! Signable command arguments and the signatures bound to them.
//!
//! Mirrors vanilla `SignableCommand` and `CommandSigningContext`. The client signs each
//! signable argument of a command individually (`ServerboundChatCommandSignedPacket`), and
//! the server binds the verified messages to the command source so the executing command can
//! deliver them as signed chat.
//!
//! Argument signability is a property of the argument parser, not of the command: any command
//! that takes a [`SteelArgumentType::message`](super::SteelArgumentType::message) argument
//! participates automatically.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::player::chat::LastSeen;

use super::{ExecutionCommandSource, SteelContextChain, argument::MessageValue};
use crate::command::brigadier::CommandContext;

/// One argument of a parsed command whose value the client may sign.
///
/// Equivalent to `SignableCommand.Argument`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SignableArgument {
    /// The Brigadier argument name the client signs under.
    pub(crate) name: String,
    /// The raw argument text that was signed.
    pub(crate) value: String,
}

/// Collects every signable argument of a parsed command, in parse order.
///
/// Equivalent to `SignableCommand.of(ParseResults)`.
pub(crate) fn signable_arguments<S>(chain: &SteelContextChain<S>) -> Vec<SignableArgument>
where
    S: ExecutionCommandSource,
{
    chain
        .remaining_contexts()
        .flat_map(CommandContext::arguments)
        .filter_map(|(name, value)| {
            value
                .downcast_ref::<MessageValue>()
                .map(|message| SignableArgument {
                    name: name.to_owned(),
                    value: message.text().to_owned(),
                })
        })
        .collect()
}

/// The verified signed message the client bound to one signable command argument.
///
/// Equivalent to the `PlayerChatMessage` vanilla stores per argument in
/// `CommandSigningContext.SignedArguments`. Everything here has already been validated
/// against the sender's profile key and message chain; `signature` is `None` only when the
/// server accepted the argument unsigned (secure chat not enforced).
#[derive(Clone, Debug)]
pub(crate) struct SignedArgument {
    signature: Option<Arc<[u8; 256]>>,
    /// The raw argument text that was signed.
    content: String,
    /// Client timestamp, in milliseconds since the epoch, that was signed.
    timestamp: i64,
    /// Client salt that was signed.
    salt: i64,
    /// The signatures the sender acknowledged alongside this message.
    last_seen: LastSeen,
}

impl SignedArgument {
    pub(crate) const fn new(
        signature: Option<Arc<[u8; 256]>>,
        content: String,
        timestamp: i64,
        salt: i64,
        last_seen: LastSeen,
    ) -> Self {
        Self {
            signature,
            content,
            timestamp,
            salt,
            last_seen,
        }
    }

    /// Returns the raw argument text covered by the signature.
    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    /// Returns the verified signature, if this argument was signed.
    pub(crate) fn signature(&self) -> Option<&[u8; 256]> {
        self.signature.as_deref()
    }

    pub(crate) const fn timestamp(&self) -> i64 {
        self.timestamp
    }

    pub(crate) const fn salt(&self) -> i64 {
        self.salt
    }

    pub(crate) const fn last_seen(&self) -> &LastSeen {
        &self.last_seen
    }
}

/// Verified signatures bound to a command's signable arguments.
///
/// Equivalent to `CommandSigningContext.SignedArguments`. Commands executed from a source
/// with no signing context (console, RCON, `/execute`, or an unsigned
/// `ServerboundChatCommandPacket`) see an empty context and deliver unsigned messages.
#[derive(Debug, Default)]
pub(crate) struct CommandSigningContext {
    arguments: FxHashMap<String, SignedArgument>,
}

impl CommandSigningContext {
    pub(crate) const fn new(arguments: FxHashMap<String, SignedArgument>) -> Self {
        Self { arguments }
    }

    /// Returns the signature bound to `name`, if the argument was signed.
    pub(crate) fn argument(&self, name: &str) -> Option<&SignedArgument> {
        self.arguments.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandSigningContext, LastSeen, SignedArgument};
    use rustc_hash::FxHashMap;
    use std::sync::Arc;

    #[test]
    fn signing_context_exposes_only_bound_arguments() {
        assert!(
            CommandSigningContext::default()
                .argument("message")
                .is_none()
        );

        let mut arguments = FxHashMap::default();
        arguments.insert(
            "message".to_owned(),
            SignedArgument::new(
                Some(Arc::new([7u8; 256])),
                "hello".to_owned(),
                42,
                7,
                LastSeen::default(),
            ),
        );
        arguments.insert(
            "unsigned".to_owned(),
            SignedArgument::new(None, String::new(), 0, 0, LastSeen::default()),
        );
        let context = CommandSigningContext::new(arguments);

        let message = context.argument("message").expect("message is bound");
        assert_eq!(message.signature(), Some(&[7u8; 256]));
        assert_eq!(message.content(), "hello");
        assert_eq!(message.timestamp(), 42);
        assert_eq!(message.salt(), 7);
        assert!(message.last_seen().is_empty());
        assert!(
            context
                .argument("unsigned")
                .is_some_and(|argument| argument.signature().is_none())
        );
        assert!(context.argument("missing").is_none());
    }
}
