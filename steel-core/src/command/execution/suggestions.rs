//! Suggestion helpers shared by command arguments

use steel_utils::Identifier;

use crate::command::brigadier::{
    ArgumentSuggestionContext, CommandArgumentParser, SuggestionProvider, SuggestionsBuilder,
};

/// Characters seperating the segments [`matches_substring`] searches
const MATCH_SPLITTER: [char; 3] = ['.', '_', '/'];

/// Returns whether `input` starts with `pattern` at the beginning of the input
/// or of any segment separated by a [`MATCH_SPLITTER`] character.
pub(in crate::command::execution) fn matches_substring(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        MATCH_SPLITTER.contains(&character)
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

/// [`matches_substring`] on lowercase inputs, as vanilla's
/// `SharedSuggestionProvider.suggest` does before filtering.
pub(in crate::command::execution) fn matches_substring_ignore_case(
    pattern: &str,
    input: &str,
) -> bool {
    matches_substring(&pattern.to_lowercase(), &input.to_lowercase())
}

/// Matches `pattern` against a whole identifier or, when it carries no
/// namespace, against its namespace and its path separately.
pub(in crate::command::execution) fn identifier_matches(
    pattern: &str,
    identifier: &Identifier,
) -> bool {
    if pattern.contains(':') {
        matches_substring(pattern, &identifier.to_string())
    } else {
        matches_substring(pattern, identifier.namespace.as_ref())
            || matches_substring(pattern, identifier.path.as_ref())
    }
}

/// Suggests the identifiers matching the remaining input.
pub(in crate::command::execution) fn suggest_resources<'a>(
    resources: impl Iterator<Item = &'a Identifier>,
    builder: &mut SuggestionsBuilder<'_>,
) {
    let contents = builder.remaining_lowercase();
    let suggestions = resources
        .filter(|resource| identifier_matches(contents, resource))
        .map(Identifier::to_string)
        .collect::<Vec<_>>();
    for suggestion in suggestions {
        builder.suggest(suggestion);
    }
}

/// Suggests every value whose name matches the remaining input, keeping the
/// values' own casing.
pub(crate) fn suggest_list<T: AsRef<str>>(
    builder: &mut SuggestionsBuilder<'_>,
    values: impl IntoIterator<Item = T>,
) {
    let lower_prefix = builder.remaining_lowercase().to_owned();
    for value in values {
        let value = value.as_ref();
        if matches_substring(&lower_prefix, &value.to_lowercase()) {
            builder.suggest(value);
        }
    }
}

/// An implementation of [`SuggestionProvider`] that suggests a constant array of suggestions.
pub(crate) struct FixedSuggestionProvider {
    suggestions: &'static [&'static str],
}

impl FixedSuggestionProvider {
    pub const fn new(suggestions: &'static [&'static str]) -> Self {
        Self { suggestions }
    }
}

impl<S, A: CommandArgumentParser<S>> SuggestionProvider<S, A> for FixedSuggestionProvider {
    fn list_suggestions(
        &self,
        _context: &ArgumentSuggestionContext<'_, S, A::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        suggest_list(builder, self.suggestions.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::brigadier::Suggestion;

    #[test]
    fn suggest_list_matches_segment_starts_case_insensitively() {
        let Ok(mut builder) = SuggestionsBuilder::new("adv SLA", 4) else {
            panic!("suggestion start should be valid");
        };
        suggest_list(
            &mut builder,
            ["Slab", "stone_slab", "Stone", "cobble_slab", "Restone"],
        );

        let Ok(suggestions) = builder.build() else {
            panic!("builder ranges should remain valid");
        };
        let texts = suggestions
            .list()
            .iter()
            .map(Suggestion::text)
            .collect::<Vec<_>>();

        assert_eq!(texts, ["cobble_slab", "Slab", "stone_slab"]);
    }
}
