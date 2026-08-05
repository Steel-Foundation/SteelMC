use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::config::C_DISCONNECT;
use steel_registry::packets::play::C_DISCONNECT as PLAY_C_DISCONNECT;
use text_components::TextComponent;
use text_components::resolving::TextResolutor;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Config = C_DISCONNECT, Play = PLAY_C_DISCONNECT)]
pub struct CDisconnect {
    pub reason: TextComponent,
}

impl CDisconnect {
    #[must_use]
    pub fn new<T: TextResolutor>(reason: &TextComponent, player: &T) -> Self {
        Self {
            reason: reason.resolve(player),
        }
    }
}
