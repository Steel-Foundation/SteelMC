//! *Suggestion providers* to provide custom suggestions for arguments when needed.

use crate::command::brigadier::{
    ArgumentSuggestionContext, CommandArgumentParser, SuggestionProvider, SuggestionsBuilder,
};

pub fn matches_suggestion_substring_case_sensitive(pattern: &str, input: &str) -> bool {
    if input.starts_with(pattern) {
        return true;
    }
    input.char_indices().any(|(index, character)| {
        matches!(character, '.' | '_' | '/')
            && input[index + character.len_utf8()..].starts_with(pattern)
    })
}

pub fn matches_suggestion_substring(pattern: &str, input: &str) -> bool {
    matches_suggestion_substring_case_sensitive(&pattern.to_lowercase(), &input.to_lowercase())
}

/// An implementation of [`SuggestionProvider`] that suggests a constant array of suggestions.
pub(crate) struct FixedSuggestionProvider {
    suggestions: &'static [&'static str],
}

impl FixedSuggestionProvider {
    pub const fn new(suggestions: &'static [&str]) -> Self {
        Self { suggestions }
    }
}

impl<S, A: CommandArgumentParser<S>> SuggestionProvider<S, A> for FixedSuggestionProvider {
    fn list_suggestions(
        &self,
        _context: &ArgumentSuggestionContext<'_, S, A::Value>,
        builder: &mut SuggestionsBuilder<'_>,
    ) {
        let lower_prefix = builder.remaining_lowercase().to_string();
        for suggestion in self.suggestions {
            if matches_suggestion_substring(&lower_prefix, suggestion) {
                builder.suggest(*suggestion);
            }
        }
    }
}
