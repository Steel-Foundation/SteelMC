//! Login state packet handlers.

use sha1::Sha1;
use sha2::Digest;
use steel_core::{player::GameProfile, server::DuplicatePlayerWaitError};
use steel_crypto::key_store::{DecryptError, KeyStore};
use steel_protocol::{
    packets::login::{CHello, CLoginCompression, CLoginFinished, SHello, SKey},
    utils::ConnectionProtocol,
};
use steel_utils::translations;
use text_components::TextComponent;

use crate::{
    AuthError, is_valid_player_name, mojang_authenticate, offline_uuid, signed_bytes_be_to_hex,
    tcp_client::{ConnectionAction, ConnectionUpdate, JavaTcpClient},
};

impl JavaTcpClient {
    async fn disconnect_duplicate_player(&self, profile: &GameProfile) -> bool {
        let Some(login_deadline) = self.login_deadline.load() else {
            log::error!(
                "Client {} reached duplicate login handling without a deadline",
                self.id
            );
            self.close();
            return false;
        };

        match self
            .server
            .disconnect_duplicate_player_and_wait(
                profile.id,
                &self.cancel_token,
                login_deadline.expires_at_tick(),
            )
            .await
        {
            Ok(()) => true,
            Err(DuplicatePlayerWaitError::Cancelled) => {
                self.close();
                false
            }
            Err(DuplicatePlayerWaitError::TimedOut) => false,
        }
    }

    async fn finish_verified_login(
        &self,
        profile: GameProfile,
        reader_encryption: Option<[u8; 16]>,
    ) -> ConnectionAction {
        // Reject full-server logins before evicting an existing session.
        if self.server.is_player_limit_reached(profile.id) {
            self.kick(TextComponent::translated(
                translations::MULTIPLAYER_DISCONNECT_SERVER_FULL.msg(),
            ))
            .await;
            return ConnectionAction::none();
        }
        let action = self.send_login_compression().await;
        if !self.disconnect_duplicate_player(&profile).await {
            return ConnectionAction::none();
        }
        self.send_login_finished(&profile).await;
        let sequence_result = self.pre_play_state.lock().complete_login(profile);
        if let Err(error) = sequence_result {
            return self.reject_unexpected_packet(error).await;
        }
        match reader_encryption {
            Some(key) => action.with_reader_encryption(key),
            None => action,
        }
    }

    /// Handles the hello packet during the login state.
    pub(crate) async fn handle_hello(&self, packet: SHello) -> ConnectionAction {
        // The hello UUID is client supplied; only authentication or offline derivation is trusted.
        let requested_username = packet.name;
        if !is_valid_player_name(&requested_username) {
            self.kick("Invalid player name".into()).await;
            return ConnectionAction::none();
        }

        if self.server.config.encryption {
            let sequence_result = self.pre_play_state.lock().wait_for_key(requested_username);
            if let Err(error) = sequence_result {
                return self.reject_unexpected_packet(error).await;
            }

            let challenge: [u8; 4] = rand::random();
            self.challenge.store(challenge);

            self.send_bare_packet_now(CHello::new(
                String::new(),
                &self.server.key_store.public_key_der,
                challenge,
                self.server.config.online_mode,
            ))
            .await;
            return ConnectionAction::none();
        }

        let profile = GameProfile {
            id: offline_uuid(&requested_username),
            name: requested_username,
            properties: vec![],
            profile_actions: None,
        };
        self.finish_verified_login(profile, None).await
    }

    /// Handles the key packet during the login state, used for encryption.
    pub(crate) async fn handle_key(&self, packet: SKey) -> ConnectionAction {
        let sequence_result = self.pre_play_state.lock().begin_authentication();
        let requested_username = match sequence_result {
            Ok(requested_username) => requested_username,
            Err(error) => return self.reject_unexpected_packet(error).await,
        };
        let challenge = self.challenge.load();

        let Ok(secret_key) =
            Self::decrypt_shared_secret(&self.server.key_store, &packet, challenge)
        else {
            self.kick("Invalid key".into()).await;
            return ConnectionAction::none();
        };

        let Ok(_) = self
            .connection_updates
            .send(ConnectionUpdate::EnableEncryption(secret_key))
        else {
            self.kick("Failed to send connection update".into()).await;
            return ConnectionAction::none();
        };

        tokio::select! {
            () = self.connection_updated.notified() => {}
            () = self.cancel_token.cancelled() => return ConnectionAction::none(),
        }

        let profile = if self.server.config.online_mode {
            let server_hash = &Sha1::new()
                .chain_update(secret_key)
                .chain_update(&self.server.key_store.public_key_der)
                .finalize();

            let server_hash = signed_bytes_be_to_hex(server_hash);

            match mojang_authenticate(
                &requested_username,
                &server_hash,
                self.server.config.auth_server.as_deref(),
            )
            .await
            {
                Ok(profile) => profile,
                Err(error) => {
                    self.kick(match error {
                        AuthError::FailedResponse => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_AUTHSERVERS_DOWN.msg(),
                        ),
                        AuthError::UnverifiedUsername => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_UNVERIFIED_USERNAME.msg(),
                        ),
                        AuthError::InvalidAuthServer(auth_server) => {
                            log::error!(
                                "Invalid authentication server URL configured: {auth_server}"
                            );
                            TextComponent::translated(
                                translations::MULTIPLAYER_DISCONNECT_AUTHSERVERS_DOWN.msg(),
                            )
                        }
                        e => e.to_string().into(),
                    })
                    .await;
                    return ConnectionAction::none();
                }
            }
        } else {
            GameProfile {
                id: offline_uuid(&requested_username),
                name: requested_username,
                properties: vec![],
                profile_actions: None,
            }
        };

        self.finish_verified_login(profile, Some(secret_key)).await
    }

    /// Decrypts the shared secret and checks the challenge response.
    ///
    /// Every failure must get the same disconnect to avoid a padding oracle.
    /// Both are decrypted up front so rejections cost equal RSA work (not constant time).
    fn decrypt_shared_secret(
        key_store: &KeyStore,
        packet: &SKey,
        challenge: [u8; 4],
    ) -> Result<[u8; 16], DecryptError> {
        let challenge_response = key_store.decrypt(&packet.challenge);
        let secret_key = key_store.decrypt(&packet.key);

        if challenge_response? != challenge {
            return Err(DecryptError);
        }

        secret_key?.try_into().map_err(|_| DecryptError)
    }

    /// Negotiates packet compression before the successful login response.
    ///
    /// # Panics
    /// This function will panic if the compression threshold cannot be converted to an i32.
    async fn send_login_compression(&self) -> ConnectionAction {
        let mut action = ConnectionAction::none();
        if let Some(compression) = self.server.config.compression {
            self.send_bare_packet_now(CLoginCompression::new(
                compression
                    .threshold
                    .get()
                    .try_into()
                    .expect("Failed to convert compression threshold to i32"),
            ))
            .await;
            self.compression.store(Some(compression));
            action = ConnectionAction::reader_compression(compression);
        }

        action
    }

    /// Sends the successful login response.
    async fn send_login_finished(&self, profile: &GameProfile) {
        self.send_bare_packet_now(CLoginFinished::new(
            profile.into(),
            self.connection_session.session_id(),
        ))
        .await;
    }

    /// Handles the login acknowledged packet and transitions to the configuration state.
    pub(crate) async fn handle_login_acknowledged(&self) -> ConnectionAction {
        let sequence_result = self.pre_play_state.lock().acknowledge_login();
        if let Err(error) = sequence_result {
            return self.reject_unexpected_packet(error).await;
        }
        self.login_deadline.store(None);
        self.protocol.store(ConnectionProtocol::Config);

        self.start_configuration().await;
        ConnectionAction::none()
    }
}

#[cfg(test)]
mod tests {
    use rsa::Pkcs1v15Encrypt;

    use super::*;

    const CHALLENGE: [u8; 4] = [1, 2, 3, 4];
    const SECRET: [u8; 16] = [7; 16];

    fn encrypt(key_store: &KeyStore, data: &[u8]) -> Vec<u8> {
        steel_crypto::public_key_from_bytes(&key_store.public_key_der)
            .expect("server public key should parse")
            .encrypt(&mut rand::rng(), Pkcs1v15Encrypt, data)
            .expect("encryption should succeed")
    }

    #[test]
    fn decrypt_shared_secret_accepts_valid_packet() {
        let key_store = KeyStore::create();
        let packet = SKey {
            key: encrypt(&key_store, &SECRET),
            challenge: encrypt(&key_store, &CHALLENGE),
        };

        let secret = JavaTcpClient::decrypt_shared_secret(&key_store, &packet, CHALLENGE)
            .expect("valid packet should decrypt");
        assert_eq!(secret, SECRET);
    }

    /// A bad padding and a wrong challenge must be indistinguishable to the caller,
    /// otherwise `handle_key` can report them differently and reopen the padding oracle.
    #[test]
    fn decrypt_shared_secret_rejects_all_failures_alike() {
        let key_store = KeyStore::create();
        let cases = [
            (vec![0; 128], encrypt(&key_store, &CHALLENGE)),
            (encrypt(&key_store, &SECRET), vec![0; 128]),
            (
                encrypt(&key_store, &SECRET),
                encrypt(&key_store, &[9, 9, 9, 9]),
            ),
            (
                encrypt(&key_store, &[7; 8]),
                encrypt(&key_store, &CHALLENGE),
            ),
        ];

        for (key, challenge) in cases {
            let packet = SKey { key, challenge };
            let result = JavaTcpClient::decrypt_shared_secret(&key_store, &packet, CHALLENGE);
            assert!(matches!(result, Err(DecryptError)));
        }
    }
}
