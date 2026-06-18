//! Login state packet handlers.

use rsa::Pkcs1v15Encrypt;
use sha1::Sha1;
use sha2::Digest;
use steel_core::player::GameProfile;
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
    /// Handles the hello packet during the login state.
    ///
    /// # Panics
    /// This function will panic if the player name converted to a UUID fails.
    pub(crate) async fn handle_hello(&self, packet: SHello) -> ConnectionAction {
        if !is_valid_player_name(&packet.name) {
            self.kick("Invalid player name".into()).await;
            return ConnectionAction::none();
        }

        let id = if self.server.config.online_mode {
            packet.profile_id
        } else {
            offline_uuid(&packet.name).expect("Failed to generate offline UUID")
        };

        {
            let mut gameprofile = self.gameprofile.lock().await;
            *gameprofile = Some(GameProfile {
                id,
                name: packet.name.clone(),
                properties: vec![],
                profile_actions: None,
            });
        }

        if self.server.config.encryption {
            let challenge: [u8; 4] = rand::random();
            self.challenge.store(challenge);

            self.send_bare_packet_now(CHello::new(
                String::new(),
                &self.server.key_store.public_key_der,
                challenge,
                true,
            ))
            .await;
        } else {
            return self
                .finish_login(&GameProfile {
                    id,
                    name: packet.name,
                    properties: vec![],
                    profile_actions: None,
                })
                .await;
        }

        ConnectionAction::none()
    }

    /// Handles the key packet during the login state, used for encryption.
    pub(crate) async fn handle_key(&self, packet: SKey) -> ConnectionAction {
        let challenge = self.challenge.load();

        let Ok(challenge_response) = self
            .server
            .key_store
            .private_key
            .decrypt(Pkcs1v15Encrypt, &packet.challenge)
        else {
            self.kick("Invalid key".into()).await;
            return ConnectionAction::none();
        };

        if challenge_response != challenge {
            self.kick("Invalid challenge response".into()).await;
            return ConnectionAction::none();
        }

        let Ok(secret_key) = self
            .server
            .key_store
            .private_key
            .decrypt(Pkcs1v15Encrypt, &packet.key)
        else {
            self.kick("Invalid key".into()).await;
            return ConnectionAction::none();
        };

        let secret_key: [u8; 16] = if let Ok(secret_key) = secret_key.try_into() {
            secret_key
        } else {
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

        let mut gameprofile = self.gameprofile.lock().await;

        let Some(profile) = gameprofile.as_mut() else {
            self.kick("No GameProfile".into()).await;
            return ConnectionAction::none();
        };

        if self.server.config.online_mode {
            let server_hash = &Sha1::new()
                .chain_update(secret_key)
                .chain_update(&self.server.key_store.public_key_der)
                .finalize();

            let server_hash = signed_bytes_be_to_hex(server_hash);

            match mojang_authenticate(&profile.name, &server_hash).await {
                Ok(new_profile) => *profile = new_profile,
                Err(error) => {
                    self.kick(match error {
                        AuthError::FailedResponse => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_AUTHSERVERS_DOWN.msg(),
                        ),
                        AuthError::UnverifiedUsername => TextComponent::translated(
                            translations::MULTIPLAYER_DISCONNECT_UNVERIFIED_USERNAME.msg(),
                        ),
                        e => e.to_string().into(),
                    })
                    .await;
                    return ConnectionAction::none();
                }
            }
        }

        //TODO: Check for duplicate player UUID or name

        self.finish_login(profile)
            .await
            .with_reader_encryption(secret_key)
    }

    /// Finishes the login process and transitions to the configuration state.
    ///
    /// # Panics
    /// This function will panic if the compression threshold cannot be converted to an i32.
    pub(crate) async fn finish_login(&self, profile: &GameProfile) -> ConnectionAction {
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

        self.send_bare_packet_now(CLoginFinished::new(
            profile.into(),
            self.connection_session.session_id(),
        ))
        .await;

        action
    }

    /// Handles the login acknowledged packet and transitions to the configuration state.
    pub async fn handle_login_acknowledged(&self) {
        self.protocol.store(ConnectionProtocol::Config);

        self.start_configuration().await;
    }
}
