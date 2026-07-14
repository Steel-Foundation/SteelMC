use crate::{
    SERVER,
    logger::{Move, output::Output, terminal_height},
};
use crossterm::{
    cursor::{MoveUp, RestorePosition, SavePosition},
    style::{
        Color::{DarkGrey, Yellow},
        ResetColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType, ScrollUp},
};
use std::{
    io::{Result, Write},
    ops::Range,
};
use steel_core::command::sender::CommandSender;

pub(super) struct Completion {
    pub(super) range: Range<usize>,
    pub(super) text: String,
}

impl Completion {
    pub(super) fn apply(self, input: &mut String) -> (usize, usize) {
        let original_length = input.chars().count();
        let replacement_length = self.range.end - self.range.start;
        let text_length = self.text.chars().count();
        let start = char_position_to_byte(input, self.range.start);
        let end = char_position_to_byte(input, self.range.end);
        input.replace_range(start..end, &self.text);
        (
            original_length - replacement_length + text_length,
            self.range.start + text_length,
        )
    }
}

pub struct Completer {
    pub enabled: bool,
    pub error: bool,
    pub selected: usize,
    suggestions: Vec<Completion>,
    reserved_rows: usize,
}
impl Completer {
    pub const fn new() -> Self {
        Completer {
            enabled: false,
            error: false,
            selected: 0,
            suggestions: vec![],
            reserved_rows: 0,
        }
    }
}
/// Modify suggestions
impl Completer {
    pub fn update(&mut self, out: &mut Output, pos: usize) {
        if !self.enabled {
            self.suggestions.clear();
            self.selected = 0;
            self.error = false;
            return;
        }
        let char_start = if pos == 0 {
            0
        } else {
            let (start, size) = out.char_pos(pos.saturating_sub(1));
            start + size
        };
        // Gets the right chars
        let command = &out.text[..char_start];

        let Some(server) = SERVER.get() else {
            self.suggestions.clear();
            self.selected = 0;
            self.error = true;
            return;
        };
        self.suggestions = server
            .command_completions(CommandSender::Console, command)
            .into_iter()
            .filter_map(|completion| {
                let start = utf16_position_to_char(command, completion.replacement_start())?;
                let end = completion
                    .replacement_start()
                    .checked_add(completion.replacement_length())?;
                let end = utf16_position_to_char(command, end)?;
                Some(Completion {
                    range: start..end,
                    text: completion.text().to_owned(),
                })
            })
            .collect();
        if self.suggestions.is_empty() {
            self.selected = 0;
            self.error = true;
        } else {
            self.selected = self.selected.min(self.suggestions.len() - 1);
            self.error = false;
        }
    }

    pub(super) fn take_selected(&mut self) -> Option<Completion> {
        if self.selected >= self.suggestions.len() {
            self.selected = 0;
            return None;
        }
        let completion = self.suggestions.swap_remove(self.selected);
        self.suggestions.clear();
        self.selected = 0;
        Some(completion)
    }

    pub(super) const fn consume_reserved_rows(&mut self, rows: usize) {
        self.reserved_rows = self.reserved_rows.saturating_sub(rows);
    }

    pub fn rewrite(&mut self, out: &mut Output, dir: Move) -> Result<()> {
        out.cursor_to_relative(out.pos)?;
        if out.is_at_end() {
            write!(out, "{}", Clear(ClearType::UntilNewLine))?;
        }
        if self.suggestions.is_empty() {
            write!(out, "{}", Clear(ClearType::FromCursorDown))?;
            out.flush()?;
            return Ok(());
        }

        // Updates completion position
        let len = self.suggestions.len();
        match dir {
            Move::Up => self.selected = (self.selected + len - 1) % len,
            Move::Down => self.selected = (self.selected + 1) % len,
            Move::None => (),
        }

        // Updates the screen width
        let width = (super::terminal_width() / 20).max(1);
        let max_height = 3.min(terminal_height().saturating_sub(4));
        let completion_height = self.suggestions.len().div_ceil(width).min(max_height);
        let grid_size = width * completion_height;
        if grid_size == 0 {
            out.flush()?;
            return Ok(());
        }
        let missing_rows = completion_height.saturating_sub(self.reserved_rows);
        reserve_completion_rows(out, missing_rows)?;
        self.reserved_rows = self.reserved_rows.max(completion_height);
        write!(out, "{SavePosition}\r\n")?;
        write!(out, "{}", Clear(ClearType::FromCursorDown))?;
        let start = (self.selected.checked_div(grid_size).unwrap_or(0)) * grid_size;
        for h in 0..completion_height {
            write!(out, "\r")?;
            for w in 0..width {
                let pos = start + w * completion_height + h;
                if pos >= self.suggestions.len() {
                    break;
                }

                let color = if pos == self.selected {
                    Yellow
                } else {
                    DarkGrey
                };

                write!(
                    out,
                    "{}{:<20}{}",
                    SetForegroundColor(color),
                    display_suggestion(&self.suggestions[pos].text),
                    ResetColor
                )?;
            }
            if h + 1 < completion_height {
                writeln!(out)?;
            }
        }
        write!(out, "{RestorePosition}")?;

        let char_pos = if out.is_at_start() {
            0
        } else {
            let (pos, char) = out.char_pos(out.pos.saturating_sub(1));
            pos + char
        };
        let selected = &self.suggestions[self.selected];
        let completed = if selected.range.end == out.pos {
            let start = char_position_to_byte(&out.text, selected.range.start);
            selected.text.strip_prefix(&out.text[start..char_pos])
        } else {
            None
        };
        out.flush()?;

        if !out.is_at_end() {
            return Ok(());
        }
        if let Some(completed) = completed {
            write!(
                out,
                "{SavePosition}{}{completed}{RestorePosition}",
                SetForegroundColor(DarkGrey),
            )?;
        }
        out.flush()
    }
}

fn char_position_to_byte(input: &str, position: usize) -> usize {
    input
        .char_indices()
        .nth(position)
        .map_or(input.len(), |(byte, _)| byte)
}

fn display_suggestion(suggestion: &str) -> String {
    if suggestion.chars().count() > 20 {
        format!("{}...", suggestion.chars().take(17).collect::<String>())
    } else {
        suggestion.to_owned()
    }
}

fn utf16_position_to_char(input: &str, position: usize) -> Option<usize> {
    let mut utf16_index = 0;
    for (char_index, character) in input.chars().enumerate() {
        if utf16_index == position {
            return Some(char_index);
        }
        utf16_index += character.len_utf16();
        if utf16_index > position {
            return None;
        }
    }
    (utf16_index == position).then_some(input.chars().count())
}

fn reserve_completion_rows(out: &mut Output, rows: usize) -> Result<()> {
    let Ok(rows) = u16::try_from(rows) else {
        return Ok(());
    };
    if rows > 0 {
        write!(out, "{}{}", ScrollUp(rows), MoveUp(rows))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Completer, Completion, display_suggestion, utf16_position_to_char};

    #[test]
    fn utf16_positions_map_to_character_positions() {
        assert_eq!(utf16_position_to_char("a😀b", 0), Some(0));
        assert_eq!(utf16_position_to_char("a😀b", 1), Some(1));
        assert_eq!(utf16_position_to_char("a😀b", 2), None);
        assert_eq!(utf16_position_to_char("a😀b", 3), Some(2));
        assert_eq!(utf16_position_to_char("a😀b", 4), Some(3));
    }

    #[test]
    fn completion_replaces_its_range_instead_of_appending() {
        let mut slash_command = "/ti".to_owned();
        let completion = Completion {
            range: 1..3,
            text: "time".to_owned(),
        };
        assert_eq!(completion.apply(&mut slash_command), (5, 5));
        assert_eq!(slash_command, "/time");

        let mut resource_command = "give @s dia".to_owned();
        let completion = Completion {
            range: 8..11,
            text: "minecraft:diamond".to_owned(),
        };
        assert_eq!(completion.apply(&mut resource_command), (25, 25));
        assert_eq!(resource_command, "give @s minecraft:diamond");
    }

    #[test]
    fn unicode_suggestions_are_truncated_at_character_boundaries() {
        let suggestion = "é".repeat(21);
        assert_eq!(
            display_suggestion(&suggestion),
            format!("{}...", "é".repeat(17))
        );
    }

    #[test]
    fn log_rows_consume_reserved_completion_space() {
        let mut completer = Completer::new();
        completer.reserved_rows = 3;
        completer.consume_reserved_rows(2);
        assert_eq!(completer.reserved_rows, 1);
        completer.consume_reserved_rows(2);
        assert_eq!(completer.reserved_rows, 0);
    }
}
