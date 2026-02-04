use crate::{
    SERVER,
    logger::{Move, output::Output},
};
use crossterm::{
    cursor::{MoveRight, MoveUp, RestorePosition, SavePosition},
    style::{
        Color::{DarkGrey, Yellow},
        ResetColor, SetForegroundColor,
    },
    terminal::{self, Clear, ClearType},
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
        let char_start = if out.text.is_empty() {
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
            self.error = false;
        }
    }
    pub fn rewrite(&mut self, out: &mut Output, dir: Move) -> Result<()> {
        // Goes to the end
        out.cursor_to(out.get_current_pos(), out.get_end())?;
        // Clears
        write!(out, "{}", Clear(ClearType::FromCursorDown))?;
        if self.suggestions.is_empty() {
            out.cursor_to(out.get_end(), out.get_current_pos())?;
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
        let width = if let Ok((width, _)) = terminal::size() {
            width as usize / 20
        } else {
            1
        };
        let grid_size = width * 3;
        let start = (self.selected / grid_size) * grid_size;
        let mut height = 0u16;
        'outer: for w in 0..width {
            for h in 0..3 {
                let pos = start + w * 3 + h;
                if pos >= self.suggestions.len() {
                    break 'outer;
                }

                writeln!(out, "\r")?;
                if w != 0 {
                    write!(out, "{}", MoveRight(w as u16 * 20))?;
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
                height += 1;
            }
            write!(out, "{}", MoveUp(3))?;
            height = 0;
        }
        let y = height + out.get_end().1 as u16;
        let x = out.get_current_pos().0;
        if y != 0 {
            write!(out, "{}", MoveUp(y))?;
        }
        write!(out, "\r{}", MoveRight(x as u16))?;

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
            &self.completed
        )?;
        out.flush()
    }
}
