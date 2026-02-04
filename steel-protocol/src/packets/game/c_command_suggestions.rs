use steel_macros::{ClientPacket, WriteTo};
#[allow(unused_imports)]
use steel_registry::packets::play::C_COMMAND_SUGGESTIONS;
use text_components::TextComponent;

/// Sent by the server in response to a command suggestion request.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_COMMAND_SUGGESTIONS)]
pub struct CCommandSuggestions {
    /// Transaction ID matching the client's request.
    #[write(as = VarInt)]
    pub id: i32,
    /// Start position in the command string where suggestions apply.
    #[write(as = VarInt)]
    pub start: i32,
    /// Length of the text to be replaced by the suggestion.
    #[write(as = VarInt)]
    pub length: i32,
    /// List of suggestion entries.
    #[write(as = Prefixed(VarInt))]
    pub suggestions: Vec<SuggestionEntry>,
}

/// A single command suggestion entry.
#[derive(WriteTo, Clone, Debug)]
pub struct SuggestionEntry {
    /// The suggestion text to insert.
    #[write(as = Prefixed(VarInt))]
    pub text: String,
    /// Optional tooltip shown when hovering over the suggestion.
    pub tooltip: Option<TextComponent>,
}

impl SuggestionEntry {
    /// Creates a new suggestion entry with just text.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tooltip: None,
        }
    }

    /// Creates a new suggestion entry with text and tooltip.
    pub fn with_tooltip(text: impl Into<String>, tooltip: impl Into<TextComponent>) -> Self {
        Self {
            text: text.into(),
            tooltip: Some(tooltip.into()),
        }
    }
}

impl CCommandSuggestions {
    /// Creates a new command suggestions response.
    pub fn new(id: i32, start: i32, length: i32, suggestions: Vec<SuggestionEntry>) -> Self {
        Self {
            id,
            start,
            length,
            suggestions,
        }
    }
}
