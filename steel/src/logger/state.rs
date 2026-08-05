use crate::config::RotationTimeFormat;
use crate::logger::file::LogFile;
use crate::logger::history::History;
use crate::logger::output::Output;
use crate::logger::selection::Selection;
use crate::logger::suggestions::Completer;
use crate::{config::LogConfig, logger::Move};
use crossterm::{
    style::{Attribute, Color, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{Clear, ClearType},
};
use std::{
    fmt::Write as _,
    fs::create_dir_all,
    io::{Result, Write},
    path::PathBuf,
};
use tokio_util::sync::CancellationToken;

pub struct LogState {
    pub out: Output,
    pub completion: Completer,
    pub history: History,
    pub selection: Selection,
    pub cancel_token: CancellationToken,
    pub file: LogFile,
}

impl LogState {
    pub async fn new(
        log_config: Option<&LogConfig>,
        cancel_token: CancellationToken,
    ) -> Result<Self> {
        let path = log_config.map_or_else(
            || PathBuf::from("./.tmp"),
            |log_config| PathBuf::from(&log_config.log_path),
        );
        let rotation_time = log_config.map_or(RotationTimeFormat::None, |l| l.rotation_time);
        let log_enabled = log_config.is_some_and(|l| l.log_file);
        let max_history = log_config.map_or(50, |l| l.max_history);

        create_dir_all(&path)?;

        Ok(LogState {
            out: Output::new(),
            completion: Completer::new(),
            history: History::new(path.clone(), max_history).await,
            selection: Selection::new(),
            cancel_token,
            file: LogFile::new(path, rotation_time, log_enabled)?,
        })
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

    pub fn apply_completion(&mut self) -> Result<()> {
        let Some(completion) = self.completion.take_selected() else {
            let position = self.out.pos;
            self.completion.update(&mut self.out, position);
            return self.rewrite_current_input();
        };
        let (length, position) = completion.apply(&mut self.out.text);
        self.completion.update(&mut self.out, position);
        self.rewrite_input(length, position)
    }
}

/// Rendering methods
impl LogState {
    pub fn rewrite_current_input(&mut self) -> Result<()> {
        self.rewrite_input(self.out.length, self.out.pos)
    }

    pub fn rewrite_input(&mut self, length: usize, pos: usize) -> Result<()> {
        self.out.cursor_to(0)?;

        let input_width = Output::visible_input_width();
        if self.out.start > pos {
            self.out.start = (pos + 1).saturating_sub(input_width);
        } else if pos.saturating_sub(self.out.start) > input_width {
            self.out.start += pos.saturating_sub(self.out.start) - input_width;
        }

        // Build the output string with selection highlighting
        let output = if self.selection.is_active() {
            let range = self.selection.get_range();
            let start = range.start;
            let end = range.end;

            let mut result = String::new();
            let mut highlighting = false;

            for (i, ch) in self.out.text.chars().enumerate() {
                if !(i >= self.out.start && i < self.out.start + input_width) {
                    continue;
                }
                let selected = i >= start && i < end;
                if selected && !highlighting {
                    highlighting = true;
                    write!(result, "{}", SetAttribute(Attribute::Reverse)).ok();
                }
                if !selected && highlighting {
                    highlighting = false;
                    write!(result, "{}", SetAttribute(Attribute::NoReverse)).ok();
                }
                result.push(ch);
            }
            if highlighting {
                write!(result, "{}", SetAttribute(Attribute::NoReverse)).ok();
            }
            result
        } else {
            self.out
                .text
                .chars()
                .skip(self.out.start)
                .take(input_width)
                .collect()
        };

        let input_color = if self.completion.error {
            SetForegroundColor(Color::Red)
        } else {
            SetForegroundColor(Color::White)
        };

        let left_arrow = if self.out.start == 0 { ">" } else { "◄" };
        let right_arrow = if self.out.start + input_width >= length {
            ""
        } else {
            " ►"
        };

        write!(
            self.out,
            "{}{left_arrow} {input_color}{}{ResetColor}{right_arrow}",
            Clear(ClearType::FromCursorDown),
            output,
        )?;

        self.out.length = length;
        self.out.pos = pos;
        self.out.cursor_to_relative(self.out.pos)?;
        self.out.flush()?;
        if self.completion.enabled {
            self.completion.rewrite(&mut self.out, Move::None)?;
        }
        Ok(())
    }
}
