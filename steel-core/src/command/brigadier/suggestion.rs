//! Command completion suggestions and UTF-16 replacement ranges.

use std::{cmp::Ordering, ops::Range};

use text_components::TextComponent;
use thiserror::Error;

use super::StringRange;

/// A suggestion range could not be mapped to valid UTF-8 boundaries.
#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum SuggestionError {
    /// The UTF-16 range is out of bounds or splits a supplementary character.
    #[error(
        "suggestion range {range:?} is invalid for input containing {input_length} UTF-16 code units"
    )]
    InvalidRange {
        range: StringRange,
        input_length: usize,
    },
    /// An expansion range does not contain the original suggestion range.
    #[error("suggestion range {outer:?} does not encompass {inner:?}")]
    NonEncompassingRange {
        outer: StringRange,
        inner: StringRange,
    },
}

/// One replacement offered for a command input range.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Suggestion {
    range: StringRange,
    text: Box<str>,
    tooltip: Option<TextComponent>,
    integer: Option<i32>,
}

impl Suggestion {
    /// Creates a textual suggestion.
    pub(crate) fn new(range: StringRange, text: impl Into<Box<str>>) -> Self {
        Self {
            range,
            text: text.into(),
            tooltip: None,
            integer: None,
        }
    }

    /// Creates a textual suggestion with a tooltip.
    pub(crate) fn with_tooltip(
        range: StringRange,
        text: impl Into<Box<str>>,
        tooltip: impl Into<TextComponent>,
    ) -> Self {
        Self {
            range,
            text: text.into(),
            tooltip: Some(tooltip.into()),
            integer: None,
        }
    }

    fn integer(range: StringRange, value: i32) -> Self {
        Self {
            range,
            text: value.to_string().into(),
            tooltip: None,
            integer: Some(value),
        }
    }

    /// Returns the replacement range.
    pub(crate) const fn range(&self) -> StringRange {
        self.range
    }

    /// Returns the replacement text.
    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    /// Returns the optional tooltip.
    pub(crate) const fn tooltip(&self) -> Option<&TextComponent> {
        self.tooltip.as_ref()
    }

    /// Applies this replacement to `input`.
    pub(crate) fn apply(&self, input: &str) -> Result<String, SuggestionError> {
        let range = Self::checked_byte_range(input, self.range)?;
        let mut result = String::with_capacity(input.len() - range.len() + self.text.len());
        result.push_str(&input[..range.start]);
        result.push_str(&self.text);
        result.push_str(&input[range.end..]);
        Ok(result)
    }

    fn expand(&self, input: &str, range: StringRange) -> Result<Self, SuggestionError> {
        if range.start() > self.range.start() || range.end() < self.range.end() {
            return Err(SuggestionError::NonEncompassingRange {
                outer: range,
                inner: self.range,
            });
        }
        if range == self.range {
            return Ok(self.clone());
        }

        let prefix = Self::checked_byte_range(
            input,
            StringRange::between(range.start(), self.range.start()),
        )?;
        let suffix =
            Self::checked_byte_range(input, StringRange::between(self.range.end(), range.end()))?;
        let mut text = String::with_capacity(prefix.len() + self.text.len() + suffix.len());
        text.push_str(&input[prefix]);
        text.push_str(&self.text);
        text.push_str(&input[suffix]);

        Ok(Self {
            range,
            text: text.into_boxed_str(),
            tooltip: self.tooltip.clone(),
            // Brigadier's expansion returns a plain Suggestion, even when the
            // original was an IntegerSuggestion.
            integer: None,
        })
    }

    fn checked_byte_range(
        input: &str,
        range: StringRange,
    ) -> Result<Range<usize>, SuggestionError> {
        range
            .byte_range(input)
            .ok_or(SuggestionError::InvalidRange {
                range,
                input_length: input.encode_utf16().count(),
            })
    }

    fn compare_ignore_case(&self, other: &Self) -> Ordering {
        match (self.integer, other.integer) {
            (Some(first), Some(second)) => first.cmp(&second),
            _ => self.text.to_lowercase().cmp(&other.text.to_lowercase()),
        }
    }
}

/// A sorted set of command completions sharing one replacement range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Suggestions {
    range: StringRange,
    suggestions: Vec<Suggestion>,
}

impl Suggestions {
    /// Creates suggestions that already share one range.
    pub(crate) const fn new(range: StringRange, suggestions: Vec<Suggestion>) -> Self {
        Self { range, suggestions }
    }

    /// Returns an empty suggestion set.
    pub(crate) const fn empty() -> Self {
        Self {
            range: StringRange::at(0),
            suggestions: Vec::new(),
        }
    }

    /// Returns the common replacement range.
    pub(crate) const fn range(&self) -> StringRange {
        self.range
    }

    /// Returns the sorted suggestions.
    pub(crate) fn list(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// Returns whether there are no suggestions.
    pub(crate) const fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }

    /// Merges multiple suggestion sets and expands them to one range.
    pub(crate) fn merge(input: &str, suggestions: Vec<Self>) -> Result<Self, SuggestionError> {
        let mut suggestions = suggestions.into_iter();
        let Some(first) = suggestions.next() else {
            return Ok(Self::empty());
        };
        let Some(second) = suggestions.next() else {
            return Ok(first);
        };

        let mut merged = first.suggestions;
        merged.extend(second.suggestions);
        for suggestions in suggestions {
            merged.extend(suggestions.suggestions);
        }
        Self::create(input, merged)
    }

    fn create(input: &str, suggestions: Vec<Suggestion>) -> Result<Self, SuggestionError> {
        if suggestions.is_empty() {
            return Ok(Self::empty());
        }

        let mut start = usize::MAX;
        let mut end = 0;
        for suggestion in &suggestions {
            start = start.min(suggestion.range.start());
            end = end.max(suggestion.range.end());
        }
        let range = StringRange::between(start, end);
        let mut expanded = Vec::with_capacity(suggestions.len());
        for suggestion in suggestions {
            let suggestion = suggestion.expand(input, range)?;
            if !expanded.contains(&suggestion) {
                expanded.push(suggestion);
            }
        }
        expanded.sort_by(Self::compare_suggestions);
        Ok(Self::new(range, expanded))
    }

    fn compare_suggestions(first: &Suggestion, second: &Suggestion) -> Ordering {
        first.compare_ignore_case(second)
    }
}

/// Accumulates suggestions for one input suffix.
pub(crate) struct SuggestionsBuilder<'input> {
    input: &'input str,
    start: usize,
    byte_start: usize,
    remaining_lowercase: String,
    suggestions: Vec<Suggestion>,
}

impl<'input> SuggestionsBuilder<'input> {
    /// Creates a builder at a UTF-16 input position.
    pub(crate) fn new(input: &'input str, start: usize) -> Result<Self, SuggestionError> {
        let range = StringRange::at(start);
        let Some(byte_range) = range.byte_range(input) else {
            return Err(SuggestionError::InvalidRange {
                range,
                input_length: input.encode_utf16().count(),
            });
        };
        Ok(Self {
            input,
            start,
            byte_start: byte_range.start,
            remaining_lowercase: input[byte_range.start..].to_lowercase(),
            suggestions: Vec::new(),
        })
    }

    /// Returns the complete input.
    pub(crate) const fn input(&self) -> &'input str {
        self.input
    }

    /// Returns the UTF-16 replacement start.
    pub(crate) const fn start(&self) -> usize {
        self.start
    }

    /// Returns the suffix being replaced.
    pub(crate) fn remaining(&self) -> &'input str {
        &self.input[self.byte_start..]
    }

    /// Returns a lowercase copy of the suffix being replaced.
    pub(crate) fn remaining_lowercase(&self) -> &str {
        &self.remaining_lowercase
    }

    /// Adds a textual suggestion unless it is already the exact suffix.
    pub(crate) fn suggest(&mut self, text: impl Into<Box<str>>) -> &mut Self {
        let text = text.into();
        if text.as_ref() != self.remaining() {
            self.suggestions.push(Suggestion::new(self.range(), text));
        }
        self
    }

    /// Adds a textual suggestion with a tooltip.
    pub(crate) fn suggest_with_tooltip(
        &mut self,
        text: impl Into<Box<str>>,
        tooltip: impl Into<TextComponent>,
    ) -> &mut Self {
        let text = text.into();
        if text.as_ref() != self.remaining() {
            self.suggestions
                .push(Suggestion::with_tooltip(self.range(), text, tooltip));
        }
        self
    }

    /// Adds an integer suggestion.
    pub(crate) fn suggest_integer(&mut self, value: i32) -> &mut Self {
        self.suggestions
            .push(Suggestion::integer(self.range(), value));
        self
    }

    /// Builds, deduplicates, and sorts the accumulated suggestions.
    pub(crate) fn build(self) -> Result<Suggestions, SuggestionError> {
        Suggestions::create(self.input, self.suggestions)
    }

    /// Creates an empty builder with the same input and start.
    pub(crate) fn restart(&self) -> Result<Self, SuggestionError> {
        Self::new(self.input, self.start)
    }

    fn range(&self) -> StringRange {
        StringRange::between(self.start, self.input.encode_utf16().count())
    }
}
