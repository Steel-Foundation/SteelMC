use crate::logger::Move;
use crate::logger::history::History;
use crate::logger::output::Output;
use crate::logger::selection::Selection;
#[cfg(feature = "spawn_chunk_display")]
use crate::logger::spawn_progress::SpawnProgressDisplay;
use crate::logger::suggestions::Completer;
use crossterm::{
    cursor::MoveLeft,
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::{
    fmt::Write as _,
    io::{Result, Write},
};
use tokio_util::sync::CancellationToken;

pub struct LogState {
    pub out: Output,
    pub completion: Completer,
    pub history: History,
    pub selection: Selection,
    #[cfg(feature = "spawn_chunk_display")]
    pub spawn_display: SpawnProgressDisplay,
    pub cancel_token: CancellationToken,
}

impl LogState {
    pub async fn new(path: &'static str, cancel_token: CancellationToken) -> Self {
        LogState {
            out: Output::new(),
            completion: Completer::new(),
            history: History::new(path).await,
            #[cfg(feature = "spawn_chunk_display")]
            spawn_display: SpawnProgressDisplay::new(),
            selection: Selection::new(),
            cancel_token,
        }
    }
}

/// Input modification methods
impl LogState {
    pub fn push(&mut self, string: String) -> Result<()> {
        if self.out.is_at_start() {
            self.out.text.insert_str(0, &string);
        } else {
            let (pos, char) = self.out.char_pos(self.out.pos.saturating_sub(1));
            self.out.text.insert_str(pos + char, &string);
        }
        let string_len = string.chars().count();
        let length = self.out.length + string_len;
        let pos = self.out.pos + string_len;
        self.completion.update(&mut self.out, pos);
        self.rewrite_input(length, pos)
    }

    pub fn replace_push(&mut self, string: String) -> Result<()> {
        if self.out.is_at_end() {
            let (pos, char) = self.out.char_pos(self.out.pos.saturating_sub(1));
            self.out.text.insert_str(pos + char, &string);
        } else {
            let (pos, char) = self.out.char_pos(self.out.pos);
            self.out.text.replace_range(pos..pos + char, &string);
        }
        let string_len = string.chars().count();
        let length = if self.out.is_at_end() {
            self.out.length + string_len
        } else {
            self.out.length + string_len.saturating_sub(1)
        };
        let pos = self.out.pos + string_len;
        self.completion.update(&mut self.out, pos);
        self.rewrite_input(length, pos)
    }

    pub fn pop_before(&mut self) -> Result<()> {
        if self.out.is_at_start() {
            return Ok(());
        }
        let (pos, _) = self.out.char_pos(self.out.pos.saturating_sub(1));
        self.out.text.remove(pos);
        let length = self.out.length - 1;
        let pos = self.out.pos - 1;
        self.completion.update(&mut self.out, pos);
        self.rewrite_input(length, pos)
    }

    pub fn pop_after(&mut self) -> Result<()> {
        if self.out.is_at_end() {
            return Ok(());
        }
        let (pos, _) = self.out.char_pos(self.out.pos);
        self.out.text.remove(pos);
        let length = self.out.length - 1;
        let pos = self.out.pos;
        self.completion.update(&mut self.out, pos);
        self.rewrite_input(length, pos)
    }

    pub fn delete_selection(&mut self) -> Result<()> {
        if !self.selection.is_active() {
            return Ok(());
        }
        let range = self.selection.get_range();
        let start = range.start;
        let end = range.end;

        // Find byte positions for the character indices
        let byte_start = self.out.char_pos(start).0;
        let char_end = self.out.char_pos(end.saturating_sub(1));
        let byte_end = char_end.0 + char_end.1;

        // Remove the selected text
        self.out.text.replace_range(byte_start..byte_end, "");

        // Update position and length
        let new_length = self.out.length - (end - start);
        let new_pos = start;
        self.selection.clear();

        // Update suggestions
        self.completion.update(&mut self.out, new_pos);
        self.rewrite_input(new_length, new_pos)
    }

    pub fn reset(&mut self) -> Result<()> {
        self.out.text = String::new();
        self.completion.enabled = false;
        self.completion.selected = 0;
        self.completion.update(&mut self.out, 0);
        self.history.pos = 0;
        self.rewrite_input(0, 0)
    }
}

/// Rendering methods
impl LogState {
    pub fn rewrite_current_input(&mut self) -> Result<()> {
        self.rewrite_input(self.out.length, self.out.pos)
    }

    pub fn rewrite_input(&mut self, length: usize, pos: usize) -> Result<()> {
        self.out.cursor_to(self.out.get_current_pos(), (0, 0))?;

        // Build the output string with selection highlighting
        let output = if self.selection.is_active() {
            let range = self.selection.get_range();
            let start = range.start;
            let end = range.end;

            let mut result = String::new();
            let mut ended = false;
            for (i, ch) in self.out.text.chars().enumerate() {
                if i == start {
                    write!(result, "{}", SetAttribute(Attribute::Reverse)).ok();
                }
                if i == end {
                    ended = true;
                    write!(result, "{}", SetAttribute(Attribute::NoReverse)).ok();
                }
                result.push(ch);
            }
            if !ended {
                write!(result, "{}", SetAttribute(Attribute::NoReverse)).ok();
            }
            result
        } else {
            self.out.text.clone()
        };

        let end_correction = if let Ok((w, _)) = terminal::size()
            && (length + 2).is_multiple_of(w as usize)
        {
            format!(" {}", MoveLeft(1))
        } else {
            String::new()
        };
        let input_color = if self.completion.error {
            SetForegroundColor(Color::Red)
        } else {
            SetForegroundColor(Color::White)
        };
        write!(
            self.out,
            "{}> {input_color}{}{end_correction}{ResetColor}",
            Clear(ClearType::FromCursorDown),
            output,
        )?;

        self.out.length = length;
        self.out.pos = pos;
        self.out
            .cursor_to(self.out.get_end(), self.out.get_current_pos())?;
        self.out.flush()?;
        if self.completion.enabled {
            self.completion.rewrite(&mut self.out, Move::None)?;
        }
        Ok(())
    }
}
