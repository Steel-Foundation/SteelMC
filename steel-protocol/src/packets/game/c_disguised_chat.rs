use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_DISGUISED_CHAT;
use text_components::{TextComponent, resolving::TextResolutor};

use super::c_player_chat::ChatTypeBound;

/// Clientbound packet for unsigned/disguised chat messages
/// This is sent when the server doesn't have a signed message
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_DISGUISED_CHAT)]
pub struct CDisguisedChat {
    pub message: TextComponent,
    pub chat_type: ChatTypeBound,
}

impl CDisguisedChat {
    pub fn new<T: TextResolutor>(
        message: &TextComponent,
        chat_type: ChatTypeBound,
        player: &T,
    ) -> Self {
        Self {
            message: message.resolve(player),
            chat_type,
        }
    }
}
