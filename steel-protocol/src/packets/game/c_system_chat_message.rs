use steel_macros::{ClientPacket, WriteTo};
#[allow(unused_imports)]
use steel_registry::packets::play::C_SYSTEM_CHAT;
use text_components::{TextComponent, resolving::TextResolutor};

#[derive(ClientPacket, WriteTo)]
#[packet_id(Play = C_SYSTEM_CHAT)]
pub struct CSystemChatMessage {
    pub content: TextComponent,
    pub overlay: bool,
}

impl CSystemChatMessage {
    pub fn new<T: TextResolutor>(content: &TextComponent, player: &T, overlay: bool) -> Self {
        CSystemChatMessage {
            content: content.resolve(player),
            overlay,
        }
    }
}
