use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SYSTEM_CHAT;
use text_components::{TextComponent, resolving::TextResolutor};

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SYSTEM_CHAT)]
pub struct CSystemChat {
    pub content: TextComponent,
    pub overlay: bool,
}

impl CSystemChat {
    pub fn new<T: TextResolutor>(content: &TextComponent, overlay: bool, player: &T) -> Self {
        Self {
            content: content.resolve(player),
            overlay,
        }
    }
}
