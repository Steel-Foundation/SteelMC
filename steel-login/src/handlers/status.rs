//! Status state packet handlers (server list ping).

use steel_core::config::STEEL_CONFIG;
use steel_protocol::packets::{
    common::{CPongResponse, SPingRequest},
    status::{CStatusResponse, Players, Status, Version},
};
use steel_registry::packets::CURRENT_MC_PROTOCOL;

use crate::tcp_client::JavaTcpClient;

impl JavaTcpClient {
    /// Handles a status request from the client.
    pub async fn handle_status_request(&self) {
        let res_packet = CStatusResponse::new(Status {
            description: &STEEL_CONFIG.motd,
            players: Some(Players {
                max: STEEL_CONFIG.max_players.cast_signed(),
                //TODO: Get online players count
                online: 0,
                sample: vec![],
            }),
            enforce_secure_chat: STEEL_CONFIG.enforce_secure_chat,
            favicon: load_favicon(),
            version: Some(Version {
                name: STEEL_CONFIG.mc_version,
                protocol: CURRENT_MC_PROTOCOL,
            }),
        });
        self.send_bare_packet_now(res_packet).await;
    }

    /// Handles a ping request from the client.
    pub async fn handle_ping_request(&self, packet: SPingRequest) {
        self.send_bare_packet_now(CPongResponse::new(packet.time))
            .await;
        self.close();
    }
}

/// Loads the favicon from config.
fn load_favicon() -> Option<String> {
    use base64::{Engine, prelude::BASE64_STANDARD};
    use std::fs;
    use std::path::Path;

    const ICON_PREFIX: &str = "data:image/png;base64,";

    if !STEEL_CONFIG.use_favicon {
        return None;
    }

    let path = Path::new(&STEEL_CONFIG.favicon);
    let Ok(icon) = fs::read(path) else {
        return None;
    };

    let cap = ICON_PREFIX.len() + icon.len().div_ceil(3) * 4;
    let mut base64 = String::with_capacity(cap);
    base64 += ICON_PREFIX;
    BASE64_STANDARD.encode_string(icon, &mut base64);
    Some(base64)
}
