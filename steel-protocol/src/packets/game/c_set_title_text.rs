use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_TITLE_TEXT;
use text_components::TextComponent;

/// Sets the title text displayed by the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_TITLE_TEXT)]
pub struct CSetTitleText {
    /// Text to display as the title.
    pub text: TextComponent,
}

impl CSetTitleText {
    /// Creates a title-text packet.
    #[must_use]
    pub fn new(text: impl Into<TextComponent>) -> Self {
        Self { text: text.into() }
    }
}
