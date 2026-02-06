use crate::SERVER;
use crate::logger::history::History;
use crate::logger::output::Output;
use crate::logger::{CommandLogger, LogState, Move};
use crossterm::{
    clipboard::CopyToClipboard,
    cursor::SetCursorStyle::{BlinkingBar, BlinkingBlock, DefaultUserShape},
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, poll, read},
    execute,
    terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode},
};
use std::time::Duration;
use std::{
    fmt::Write as _,
    io::{Result, Write},
    sync::Arc,
};
use steel_core::command::sender::CommandSender;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::spawn_blocking;

enum ExtendedKey {
    Generic(KeyEvent),
    Ctrl(char),
    String(String),
}

impl CommandLogger {
    /// Main entry of the input process
    pub async fn input_main(self: Arc<Self>) -> Result<()> {
        let (tx, rx) = mpsc::unbounded_channel();
        enable_raw_mode()?;
        self.clone().input_receiver(tx);
        let stopped = self.stopped.clone();
        let result = self.input_key(rx).await;
        stopped.cancel();
        result
    }

    fn input_receiver(self: Arc<Self>, tx: UnboundedSender<ExtendedKey>) {
        spawn_blocking(move || {
            let mut string = String::new();
            loop {
                if self.cancel_token.is_cancelled() {
                    break;
                }

                if let Ok(true) = poll(Duration::from_millis(50)) {
                    let event = read().expect("Event bug; Cannot read event.");
                    // On Windows, crossterm sends both Press and Release events.
                    // Only handle Press events to avoid duplicate input.
                    if let Event::Key(key) = event {
                        if key.kind != KeyEventKind::Press {
                            continue;
                        }
                        if let KeyCode::Char(char) = key.code {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                tx.send(ExtendedKey::Ctrl(char)).ok();
                            } else {
                                write!(string, "{char}").ok();
                            }
                            continue;
                        }
                        tx.send(ExtendedKey::Generic(key)).ok();
                    }
                }
                if !string.is_empty() {
                    tx.send(ExtendedKey::String(string.clone())).ok();
                    string = String::new();
                }
            }
        });
    }

    #[allow(clippy::too_many_lines)]
    async fn input_key(self: Arc<Self>, mut rx: UnboundedReceiver<ExtendedKey>) -> Result<()> {
        loop {
            tokio::select! {
                Some(key) = rx.recv() => {
                    let mut lock = self.input.write().await;
                    let state = &mut lock as &mut LogState;
                    match key {
                        ExtendedKey::Generic(key) => match key.code {
                            KeyCode::Enter => {
                                if state.out.is_empty() {
                                    continue;
                                }
                                let message = state.out.text.clone();
                                state.history.push(&state.out);
                                state.reset()?;
                                drop(lock);
                                steel_utils::console!("{}", message);
                                if let Some(server) = SERVER.get() {
                                    server.command_dispatcher.read().handle_command(
                                        CommandSender::Console,
                                        message,
                                        server,
                                    );
                                }
                                continue;
                            }
                            KeyCode::Tab => {
                                if state.completion.enabled {
                                    state.completion.enabled = false;
                                    state.completion.selected = 0;
                                    state.push(state.completion.completed.clone())?;
                                    state.completion.completed = String::new();
                                } else {
                                    state.completion.enabled = true;
                                    let pos = state.out.pos;
                                    state.completion.update(&mut state.out, pos);
                                    state.rewrite_current_input()?;
                                }
                                continue;
                            }
                            KeyCode::Backspace => {
                                if state.selection.is_active() {
                                    state.delete_selection()?;
                                    continue;
                                }
                                state.pop_before()?;
                            }
                            KeyCode::Delete => {
                                if state.selection.is_active() {
                                    state.delete_selection()?;
                                    continue;
                                }
                                state.pop_after()?;
                            }
                            KeyCode::Left if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                if !state.out.is_at_start() {
                                    if !state.selection.is_active() {
                                        state.selection.start_at(state.out.pos);
                                    }
                                    let from = state.out.get_current_pos();
                                    let to = Output::get_pos(state.out.pos - 1);
                                    state.out.cursor_to(from, to)?;
                                    state.out.pos -= 1;
                                    let new_pos = state.out.pos;
                                    state.selection.extend(new_pos);
                                    state.completion.update(&mut state.out, new_pos);
                                    state.rewrite_input(state.out.length, new_pos)?;
                                }
                                continue;
                            }
                            KeyCode::Left => {
                                if state.selection.is_active() {
                                    let pos = state.selection.get_range().start;
                                    state.selection.clear();
                                    state.completion.update(&mut state.out, pos);
                                    state.rewrite_input(state.out.length, pos)?;
                                    continue;
                                }
                                if !state.out.is_at_start() {
                                    let pos = state.out.pos - 1;
                                    let to = Output::get_pos(pos);
                                    state.out.cursor_to(state.out.get_current_pos(), to)?;
                                    state.out.pos -= 1;
                                    state.completion.update(&mut state.out, pos);
                                }
                            }
                            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                if !state.out.is_at_end() {
                                    if !state.selection.is_active() {
                                        state.selection.start_at(state.out.pos);
                                    }
                                    let from = state.out.get_current_pos();
                                    let to = Output::get_pos(state.out.pos + 1);
                                    state.out.cursor_to(from, to)?;
                                    state.out.pos += 1;
                                    let new_pos = state.out.pos;
                                    state.selection.extend(new_pos);
                                    state.completion.update(&mut state.out, new_pos);
                                    state.rewrite_input(state.out.length, new_pos)?;
                                }
                            }
                            KeyCode::Right => {
                                if state.selection.is_active() {
                                    let pos = state.selection.get_range().end;
                                    state.selection.clear();
                                    state.completion.update(&mut state.out, pos);
                                    state.rewrite_input(state.out.length, pos)?;
                                    continue;
                                }
                                if !state.out.is_at_end() {
                                    let pos = state.out.pos + 1;
                                    let to = Output::get_pos(pos);
                                    state.out.cursor_to(state.out.get_current_pos(), to)?;
                                    state.out.pos += 1;
                                    state.completion.update(&mut state.out, pos);
                                }
                            }
                            KeyCode::Up => {
                                previous(state)?;
                                continue;
                            }
                            KeyCode::Down => {
                                next(state)?;
                                continue;
                            }
                            KeyCode::End if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                // Select all text next
                                if state.out.is_at_end() {
                                    continue;
                                }
                                let len = state.out.length;
                                let start = if state.selection.is_active() {
                                    state.selection.get_range().start
                                } else {
                                    state.out.pos
                                };
                                state.selection.set(start, len);
                                state.completion.update(&mut state.out, len);
                                state.rewrite_input(len, len)?;
                                continue;
                            }
                            KeyCode::End => {
                                if state.selection.is_active() {
                                    let length = state.out.length;
                                    state.selection.clear();
                                    state.completion.update(&mut state.out, length);
                                    state.rewrite_input(length, length)?;
                                    continue;
                                }
                                if !state.out.is_at_end() {
                                    state.out.cursor_to(state.out.get_current_pos(), state.out.get_end())?;
                                    state.out.pos = state.out.length;
                                    let pos = state.out.length;
                                    state.completion.update(&mut state.out, pos);
                                }
                            }
                            KeyCode::Home if key.modifiers.contains(KeyModifiers::SHIFT) =>{
                                // Select all previous text
                                if state.out.is_at_start() {
                                    continue;
                                }
                                let end = if state.selection.is_active() {
                                    state.selection.get_range().end
                                } else {
                                    state.out.pos
                                };
                                state.selection.set(0, end + 1);
                                state.completion.update(&mut state.out, 0);
                                state.rewrite_input(state.out.length, 0)?;
                                continue;
                            }
                            KeyCode::Home => {
                                if state.selection.is_active() {
                                    state.selection.clear();
                                    state.completion.update(&mut state.out, 0);
                                    state.rewrite_input(state.out.length, 0)?;
                                    continue;
                                }
                                if !state.out.is_at_start() {
                                    state.out.cursor_to(state.out.get_current_pos(), Output::START_POS)?;
                                    state.out.pos = 0;
                                    state.completion.update(&mut state.out, 0);
                                }
                            }
                            KeyCode::Insert => {
                                state.out.replace = !state.out.replace;
                                if state.out.replace {
                                    write!(state.out, "{BlinkingBlock}")?;
                                } else {
                                    write!(state.out, "{BlinkingBar}")?;
                                }
                                continue;
                            }
                            KeyCode::Esc => {
                                state.selection.clear();
                                state.reset()?;
                                continue;
                            }
                            _ => continue,
                        },
                        ExtendedKey::Ctrl(char) => {
                            match char {
                                'c' => {
                                    if state.selection.is_active() {
                                        copy_to_clipboard(state);
                                        continue;
                                    }
                                    state.cancel_token.cancel();
                                }
                                'q' => {
                                    state.cancel_token.cancel();
                                }
                                'x' => {
                                    if state.selection.is_active() {
                                        copy_to_clipboard(state);
                                        state.delete_selection()?;
                                    }
                                    continue;
                                }
                                'a' => {
                                    // Select all text
                                    if state.out.length == 0 {
                                        continue;
                                    }
                                    let len = state.out.length;
                                    state.selection.set(0, len);
                                    state.completion.update(&mut state.out, len);
                                    state.rewrite_input(len, len)?;
                                    continue;
                                }
                                'p' => {
                                    previous(state)?;
                                    continue;
                                }
                                'n' => {
                                    next(state)?;
                                    continue;
                                }
                                _ => continue,
                            }
                        }
                        ExtendedKey::String(string) => {
                            if string.chars().any(char::is_whitespace) {
                                state.completion.selected = 0;
                            }
                            if state.selection.is_active() {
                                state.delete_selection()?;
                                state.push(string)?;
                                continue;
                            }

                            if state.out.replace {
                                state.replace_push(string)?;
                            } else {
                                state.push(string)?;
                            }
                            continue;
                        }
                    }
                    if state.completion.enabled {
                        state.completion.rewrite(&mut state.out, Move::None)?;
                    }
                    state.out.flush()?;
                }
                () = self.cancel_token.cancelled() => {
                    let mut state = self.input.write().await;
                    state.completion.enabled = false;
                    if !state.out.is_at_end() {
                        let from = state.out.get_current_pos();
                        let to = state.out.get_end();
                        state.out.cursor_to(from, to)?;
                    }
                    write!(state.out, "{}{DefaultUserShape}", Clear(ClearType::FromCursorDown))?;
                    state.history.save().await?;
                    state.out.flush()?;
                    disable_raw_mode()?;
                    break;
                },
            }
        }
        Ok(())
    }
}

fn copy_to_clipboard(input: &mut LogState) -> Option<()> {
    let range = input.selection.get_range();
    let start = range.start;
    let end = range.end;

    let byte_start = input.out.char_pos(start).0;
    let char_end = input.out.char_pos(end.saturating_sub(1));
    let byte_end = char_end.0 + char_end.1;
    let text = input.out.text[byte_start..byte_end].to_string();
    if let Err(err) = execute!(input.out, CopyToClipboard::to_clipboard_from(text)) {
        log::error!("{err}");
        return None;
    }
    Some(())
}

fn previous(state: &mut LogState) -> Result<()> {
    if state.completion.enabled {
        state.completion.rewrite(&mut state.out, Move::Up)?;
    } else {
        state.selection.clear();
        History::update(state, Move::Up)?;
    }
    Ok(())
}
fn next(state: &mut LogState) -> Result<()> {
    if state.completion.enabled {
        state.completion.rewrite(&mut state.out, Move::Down)?;
    } else {
        state.selection.clear();
        History::update(state, Move::Down)?;
    }
    Ok(())
}
