use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_SUBTITLE_TEXT;
use text_components::TextComponent;

/// Sets the subtitle text displayed by the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_SUBTITLE_TEXT)]
pub struct CSetSubtitleText {
    /// Text to display as the subtitle.
    pub text: TextComponent,
}

impl CSetSubtitleText {
    /// Creates a subtitle-text packet.
    #[must_use]
    pub fn new(text: impl Into<TextComponent>) -> Self {
        Self { text: text.into() }
    }
}
