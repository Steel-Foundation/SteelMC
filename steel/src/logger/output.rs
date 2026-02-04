use crossterm::{
    cursor::{MoveDown, MoveLeft, MoveRight, MoveUp, SetCursorStyle::BlinkingBar},
    terminal,
};
use std::io::{Result, Stdout, Write, stdout};

pub struct Output {
    pub text: String,
    pub length: usize,
    pub pos: usize,
    pub replace: bool,
    out: Stdout,
}

impl Write for Output {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        self.out.write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        self.out.flush()
    }
}

/// Constructor
impl Output {
    pub fn new() -> Self {
        let mut out = stdout();
        let _ = write!(out, "{BlinkingBar}");

        Self {
            text: String::new(),
            length: 0,
            pos: 0,
            replace: false,
            out,
        }
    }
}
/// Utilities
impl Output {
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub const fn is_at_start(&self) -> bool {
        self.pos == 0
    }
    pub const fn is_at_end(&self) -> bool {
        self.pos == self.length
    }
    pub fn char_pos(&self, pos: usize) -> (usize, usize) {
        let (pos, char) = self
            .text
            .char_indices()
            .nth(pos)
            .expect("Character position out of range!");
        (pos, char.len_utf8())
    }
    pub const START_POS: (usize, usize) = (2, 0);
    // TODO: Change the order to (x, y)
    pub fn get_pos(pos: usize) -> (usize, usize) {
        if let Ok((w, _)) = terminal::size() {
            let w = w as usize;
            let absolute_pos = pos + 2;
            let x = absolute_pos % w;
            let y = absolute_pos / w;
            return (x, y);
        }
        (pos + 2, 0)
    }
    pub fn get_current_pos(&self) -> (usize, usize) {
        Self::get_pos(self.pos)
    }
    pub fn get_end(&self) -> (usize, usize) {
        Self::get_pos(self.length)
    }
    pub fn cursor_to(&mut self, from: (usize, usize), to: (usize, usize)) -> Result<()> {
        if from.0 > to.0 {
            write!(self.out, "{}", MoveLeft((from.0 - to.0) as u16))?;
        } else if to.0 > from.0 {
            write!(self.out, "{}", MoveRight((to.0 - from.0) as u16))?;
        }
        if from.1 > to.1 {
            write!(self.out, "{}", MoveUp((from.1 - to.1) as u16))?;
        } else if to.1 > from.1 {
            write!(self.out, "{}", MoveDown((to.1 - from.1) as u16))?;
        }
        Ok(())
    }
}
