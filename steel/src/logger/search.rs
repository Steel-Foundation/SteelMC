use crate::logger::LogState;
use std::{borrow::Cow, collections::VecDeque, io::Result, ops::Range};

/// An incremental backwards search through the command history, entered with `Ctrl+R`.
pub struct ReverseSearch {
    query: String,
    /// The history entry on screen and where the query sits inside it.
    current: Option<Match>,
    /// Set when the last query change found nothing; the previous match stays on screen.
    failed: bool,
    /// Input line and cursor position restored when the search is aborted.
    original: String,
    original_pos: usize,
}

struct Match {
    entry: usize,
    range: Range<usize>,
}

impl ReverseSearch {
    pub(super) fn start(state: &mut LogState) -> Result<()> {
        let pos = state.out.pos;
        state.completion.enabled = false;
        state.completion.update(&mut state.out, pos);
        state.selection.clear();
        state.search = Some(ReverseSearch {
            query: String::new(),
            current: None,
            failed: false,
            original: state.out.text.clone(),
            original_pos: pos,
        });
        state.rewrite_current_input()
    }

    /// Steps to the next older match.
    pub(super) fn advance(state: &mut LogState) -> Result<()> {
        let Some(search) = &state.search else {
            return Ok(());
        };
        let from = search.current.as_ref().map_or(0, |found| found.entry + 1);
        Self::update(state, from)
    }

    pub(super) fn push(state: &mut LogState, text: &str) -> Result<()> {
        let Some(search) = &mut state.search else {
            return Ok(());
        };
        search.query.push_str(text);
        let from = search.current.as_ref().map_or(0, |found| found.entry);
        Self::update(state, from)
    }

    pub(super) fn pop(state: &mut LogState) -> Result<()> {
        let Some(search) = &mut state.search else {
            return Ok(());
        };
        if search.query.pop().is_none() {
            return Ok(());
        }
        let from = search.current.as_ref().map_or(0, |found| found.entry);
        Self::update(state, from)
    }

    /// Leaves search mode, keeping the matched entry as the input line.
    pub(super) fn accept(state: &mut LogState) -> Result<()> {
        let Some(search) = state.search.take() else {
            return Ok(());
        };
        if let Some(found) = search.current {
            state.history.pos = found.entry + 1;
        }
        state.rewrite_current_input()
    }

    /// Leaves search mode and restores the line the search started from.
    pub(super) fn abort(state: &mut LogState) -> Result<()> {
        let Some(search) = state.search.take() else {
            return Ok(());
        };
        let length = search.original.chars().count();
        state.out.text = search.original;
        state.rewrite_input(length, search.original_pos)
    }

    pub(super) fn prompt(&self) -> String {
        let failed = if self.failed { "failed " } else { "" };
        format!("({failed}reverse-i-search)`{}': ", self.query)
    }

    pub(super) fn match_range(&self) -> Option<Range<usize>> {
        self.current.as_ref().map(|found| found.range.clone())
    }

    /// Searches from `from` towards the oldest entry and draws the result.
    fn update(state: &mut LogState, from: usize) -> Result<()> {
        let Some(search) = &mut state.search else {
            return Ok(());
        };
        if search.query.is_empty() {
            search.current = None;
            search.failed = false;
        } else if let Some(found) = find(&state.history.values, &search.query, from) {
            search.current = Some(found);
            search.failed = false;
        } else {
            search.failed = true;
        }

        let (text, pos) = match &search.current {
            Some(found) => (
                state.history.values[found.entry].to_string(),
                found.range.start,
            ),
            None => (search.original.clone(), search.original_pos),
        };
        let length = text.chars().count();
        state.out.text = text;
        state.rewrite_input(length, pos)
    }
}

/// Finds the newest entry at or after `from` containing `query`.
fn find(values: &VecDeque<Cow<'static, str>>, query: &str, from: usize) -> Option<Match> {
    values
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(entry, value)| {
            let byte = value.find(query)?;
            let start = value[..byte].chars().count();
            Some(Match {
                entry,
                range: start..start + query.chars().count(),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::find;
    use std::{borrow::Cow, collections::VecDeque};

    fn history(values: &[&str]) -> VecDeque<Cow<'static, str>> {
        values
            .iter()
            .map(|value| Cow::Owned((*value).to_owned()))
            .collect()
    }

    #[test]
    fn searches_from_the_requested_entry_towards_older_ones() {
        let values = history(&["give @s dirt", "time set day", "give @s stone"]);
        let found = |from| find(&values, "give", from).map(|found| (found.entry, found.range));

        assert_eq!(found(0), Some((0, 0..4)));
        assert_eq!(found(1), Some((2, 0..4)));
        assert_eq!(found(3), None);
    }

    #[test]
    fn match_ranges_are_character_positions() {
        let values = history(&["say éé done"]);
        let found = find(&values, "done", 0).map(|found| found.range);
        assert_eq!(found, Some(7..11));
    }
}
