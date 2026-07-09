use steel_protocol::packets::game::{CCommandSuggestions, SuggestionEntry};

use super::brigadier::Suggestions;

const MAX_COMMAND_SUGGESTIONS: usize = 1000;

/// Converts Brigadier suggestions to the vanilla command-suggestion packet.
pub(crate) fn command_suggestions_packet(
    transaction_id: i32,
    suggestions: &Suggestions,
) -> CCommandSuggestions {
    let range = suggestions.range();
    // Serverbound command suggestions are bounded to 32,500 bytes, so their
    // UTF-16 indices always fit the packet's signed VarInts.
    let start = range.start() as i32;
    let length = range.len() as i32;
    let entries = suggestions
        .list()
        .iter()
        .take(MAX_COMMAND_SUGGESTIONS)
        .map(|suggestion| match suggestion.tooltip() {
            Some(tooltip) => SuggestionEntry::with_tooltip(suggestion.text(), tooltip.clone()),
            None => SuggestionEntry::new(suggestion.text()),
        })
        .collect();
    CCommandSuggestions::new(transaction_id, start, length, entries)
}

#[cfg(test)]
mod tests {
    use super::{MAX_COMMAND_SUGGESTIONS, command_suggestions_packet};
    use crate::command::brigadier::{
        CommandDispatcher, StringRange, StringReader, Suggestion, Suggestions, literal,
    };
    use text_components::TextComponent;

    #[test]
    fn leading_slash_remains_in_the_packet_replacement_range() {
        let mut dispatcher = CommandDispatcher::<()>::new();
        assert!(dispatcher.register(literal("help")).is_ok());
        let mut reader = StringReader::new("/he");
        assert!(reader.skip());

        let parse = dispatcher.parse_reader(reader, ());
        let suggestions = dispatcher.completion_suggestions(&parse);
        let Ok(suggestions) = suggestions else {
            panic!("slash-prefixed suggestions should build");
        };
        let packet = command_suggestions_packet(7, &suggestions);

        assert_eq!(packet.id, 7);
        assert_eq!(packet.start, 1);
        assert_eq!(packet.length, 2);
        assert_eq!(packet.suggestions.len(), 1);
        assert_eq!(packet.suggestions[0].text, "help");
    }

    #[test]
    fn packet_projection_preserves_utf16_range_and_tooltip() {
        let tooltip = TextComponent::plain("details");
        let suggestions = Suggestions::new(
            StringRange::between(2, 4),
            vec![Suggestion::with_tooltip(
                StringRange::between(2, 4),
                "value",
                tooltip.clone(),
            )],
        );

        let packet = command_suggestions_packet(11, &suggestions);

        assert_eq!(packet.start, 2);
        assert_eq!(packet.length, 2);
        assert_eq!(packet.suggestions[0].text, "value");
        assert_eq!(packet.suggestions[0].tooltip.as_ref(), Some(&tooltip));
    }

    #[test]
    fn packet_projection_applies_vanillas_suggestion_limit() {
        let range = StringRange::at(0);
        let suggestions = Suggestions::new(
            range,
            (0..=MAX_COMMAND_SUGGESTIONS)
                .map(|index| Suggestion::new(range, index.to_string()))
                .collect(),
        );

        let packet = command_suggestions_packet(1, &suggestions);

        assert_eq!(packet.suggestions.len(), MAX_COMMAND_SUGGESTIONS);
        assert_eq!(packet.suggestions[0].text, "0");
        assert_eq!(packet.suggestions[MAX_COMMAND_SUGGESTIONS - 1].text, "999");
    }
}
