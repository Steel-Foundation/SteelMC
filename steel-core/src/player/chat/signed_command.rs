//! Secure-chat verification for signed command packets.
//!
//! Mirrors `ServerGamePacketListenerImpl.handleSignedChatCommand` and
//! `collectSignedArguments`. A `ServerboundChatCommandSignedPacket` carries the same
//! security envelope as a signed chat message — timestamp, salt, last-seen acknowledgements,
//! and one signature per signable command argument — so it is validated through the same
//! session, message-chain, and acknowledgement machinery as [`super::Player::handle_chat`].
//!
//! Verification of individual argument signatures needs the parsed command (the signed
//! content is each argument's *value*, looked up by argument name), which is only available
//! on the game tick. The packet handler therefore captures the envelope into
//! [`SignedCommandPayload`] and the tick verifies it once the command has been parsed.

use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_protocol::packets::game::SChatCommandSigned;

use crate::command::execution::{CommandSigningContext, SignableArgument, SignedArgument};

use super::{message_chain, profile_key::RemoteChatSession};
use crate::player::Player;

/// The security envelope of a `ServerboundChatCommandSignedPacket`.
///
/// Kept whole rather than reduced to the command string so the signable arguments discovered
/// during parsing can each be verified against the client's signature for that argument.
#[derive(Clone, Debug)]
pub struct SignedCommandPayload {
    timestamp: i64,
    salt: i64,
    /// Argument name to the client's signature for that argument.
    signatures: FxHashMap<String, [u8; 256]>,
    acknowledged: [u8; 3],
    offset: i32,
}

impl SignedCommandPayload {
    /// Captures the envelope of a signed command packet, dropping the command text itself.
    #[must_use]
    pub fn from_packet(packet: &SChatCommandSigned) -> Self {
        let signatures = packet
            .argument_signatures
            .iter()
            .map(|entry| (entry.name.clone(), entry.signature))
            .collect();
        Self {
            timestamp: packet.timestamp,
            salt: packet.salt,
            signatures,
            acknowledged: packet.last_seen.acknowledged,
            offset: packet.last_seen.offset.0,
        }
    }
}

/// Why a signed command packet could not be accepted.
///
/// `should_disconnect` mirrors `SignedMessageChain.DecodeException.shouldDisconnect`: vanilla
/// only kicks the client when secure chat is enforced, and otherwise reports the failure to
/// the player and drops the command.
#[derive(Debug)]
pub struct SignedCommandError {
    reason: String,
    should_disconnect: bool,
}

impl SignedCommandError {
    const fn new(reason: String, should_disconnect: bool) -> Self {
        Self {
            reason,
            should_disconnect,
        }
    }

    /// Returns whether the client must be disconnected rather than merely told.
    #[must_use]
    pub const fn should_disconnect(&self) -> bool {
        self.should_disconnect
    }
}

impl fmt::Display for SignedCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason)
    }
}

impl Player {
    /// Verifies a signed command's envelope and binds a signature to each signable argument.
    ///
    /// Equivalent to `collectSignedArguments`: the last-seen acknowledgement is applied once
    /// for the packet, then every signable argument is decoded through the signed-message
    /// chain against the client's signature for that argument. An argument the client did not
    /// sign is bound unsigned, which vanilla permits unless secure chat is enforced.
    ///
    /// # Errors
    /// Returns an error when the session, acknowledgement window, message chain, or any
    /// required argument signature fails validation. Callers must not run the command in that
    /// case.
    pub(crate) fn verify_signed_command(
        &self,
        payload: &SignedCommandPayload,
        arguments: &[SignableArgument],
    ) -> Result<CommandSigningContext, SignedCommandError> {
        let enforces_secure_chat = self.server().enforces_secure_chat();
        let mut chat = self.chat.lock();

        let fail = |reason| SignedCommandError::new(reason, enforces_secure_chat);

        let timestamp = Self::signing_timestamp(payload.timestamp).map_err(fail)?;
        // The acknowledgement window advances once per packet, exactly as for signed chat,
        // so a signed command cannot desynchronize the client's last-seen state.
        let last_seen = Self::apply_last_seen_update(
            &mut chat,
            payload.acknowledged,
            payload.offset,
            // Signed command packets carry no acknowledgement checksum.
            0,
        )
        .map_err(fail)?;

        // Only look the session up when something actually needs verifying: a command with no
        // signable arguments is exactly an ordinary command, and vanilla runs it either way.
        let session: Option<RemoteChatSession> = if arguments.is_empty() {
            None
        } else {
            match Self::signing_session(&chat) {
                Ok(session) => Some(session),
                Err(error) if enforces_secure_chat => return Err(fail(error)),
                Err(_) => None,
            }
        };

        let mut signed = FxHashMap::default();
        for argument in arguments {
            let signature = payload.signatures.get(&argument.name);
            let Some((session, signature)) = session.as_ref().zip(signature) else {
                if enforces_secure_chat {
                    return Err(fail(format!(
                        "Command argument '{}' was not signed",
                        argument.name
                    )));
                }
                signed.insert(
                    argument.name.clone(),
                    SignedArgument::new(
                        None,
                        argument.value.clone(),
                        payload.timestamp,
                        payload.salt,
                        last_seen.clone(),
                    ),
                );
                continue;
            };

            let body = message_chain::SignedMessageBody::new(
                argument.value.clone(),
                timestamp,
                payload.salt,
                last_seen.clone(),
            );
            match Self::decode_signed_body(&mut chat, session, &body, signature) {
                Ok(_link) => {
                    signed.insert(
                        argument.name.clone(),
                        SignedArgument::new(
                            Some(Arc::new(*signature)),
                            argument.value.clone(),
                            payload.timestamp,
                            payload.salt,
                            last_seen.clone(),
                        ),
                    );
                }
                Err(error) => {
                    // Never fall back to "unsigned" on a signature the client did present:
                    // a failed verification means the argument was tampered with.
                    return Err(fail(format!(
                        "Command argument '{}' failed signature validation: {error}",
                        argument.name
                    )));
                }
            }
        }

        Ok(CommandSigningContext::new(signed))
    }
}
