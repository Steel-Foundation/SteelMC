use crate::{
    SERVER,
    logger::{Move, output::Output, terminal_height},
};
use crossterm::{
    cursor::{RestorePosition, SavePosition},
    style::{
        Color::{DarkGrey, Yellow},
        ResetColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType},
};
use std::io::{Result, Write};
use steel_core::command::sender::CommandSender;

pub struct Completer {
    pub enabled: bool,
    pub error: bool,
    pub selected: usize,
    pub completed: String,
    pub suggestions: Vec<String>,
}
impl Completer {
    pub const fn new() -> Self {
        Completer {
            enabled: false,
            error: false,
            selected: 0,
            completed: String::new(),
            suggestions: vec![],
        }
    }
}
/// Modify suggestions
impl Completer {
    pub fn update(&mut self, out: &mut Output, pos: usize) {
        let char_start = if pos == 0 {
            0
        } else {
            let (start, size) = out.char_pos(pos.saturating_sub(1));
            start + size
        };
        // Gets the right chars
        let command = &out.text[..char_start];

        let Some(server) = SERVER.get() else {
            self.completed = String::new();
            self.selected = 0;
            self.error = true;
            return;
        };
        // Gets the suggested commands
        self.suggestions = server
            .command_dispatcher
            .read()
            .handle_suggestions(CommandSender::Console, command, server.clone())
            .0
            .into_iter()
            .map(|suggestion| suggestion.text)
            .collect();
        if self.suggestions.is_empty() {
            self.completed = String::new();
            self.selected = 0;
            self.error = true;
        } else {
            self.selected = self.selected.min(self.suggestions.len() - 1);
            self.error = false;
        }
    }
    pub fn rewrite(&mut self, out: &mut Output, dir: Move) -> Result<()> {
        out.cursor_to_relative(out.pos)?;
        if out.is_at_end() {
            write!(out, "{}", Clear(ClearType::UntilNewLine))?;
        }
        write!(out, "{SavePosition}\r\n")?;
        write!(out, "{}", Clear(ClearType::FromCursorDown))?;
        if self.suggestions.is_empty() {
            write!(out, "{RestorePosition}")?;
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
        let completion_height = 3.min(terminal_height().saturating_sub(4));
        let grid_size = width * completion_height;
        if grid_size == 0 {
            write!(out, "{RestorePosition}")?;
            out.flush()?;
            return Ok(());
        }
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
                    if self.suggestions[pos].len() > 20 {
                        format!("{}...", &self.suggestions[pos][..17])
                    } else {
                        self.suggestions[pos].clone()
                    },
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
        let text = if let Some(text) = out.text[..char_pos].split_whitespace().last()
            && let Some(striped) = self.suggestions[self.selected].strip_prefix(text)
        {
            striped
        } else {
            &self.suggestions[self.selected]
        };
        self.completed = text.to_string();
        out.flush()?;

        if !out.is_at_end() {
            return Ok(());
        }
        write!(
            out,
            "{SavePosition}{}{}{RestorePosition}",
            SetForegroundColor(DarkGrey),
            self.completed
        )?;
        out.flush()
    }
}
