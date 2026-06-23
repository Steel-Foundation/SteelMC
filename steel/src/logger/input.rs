use crate::SERVER;
use crate::logger::history::History;
use crate::logger::{CommandLogger, LogState, Move, terminal_width};
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
use tokio::sync::RwLockWriteGuard;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::spawn_blocking;

enum ExtendedKey {
    Generic(KeyEvent),
    Ctrl(char),
    String(String),
    Resize,
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
                    match event {
                        Event::Key(key) => {
                            if key.kind != KeyEventKind::Press {
                                continue;
                            }
                            if let KeyCode::Char(char) = key.code {
                                if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    tx.send(ExtendedKey::Ctrl(char)).ok();
                                    continue;
                                }
                                write!(string, "{char}").ok();
                            } else {
                                tx.send(ExtendedKey::Generic(key)).ok();
                            }
                        }
                        Event::Resize(..) => {
                            tx.send(ExtendedKey::Resize).ok();
                        }
                        _ => (),
                    }
                }
                if !string.is_empty() {
                    tx.send(ExtendedKey::String(string.clone())).ok();
                    string = String::new();
                }
            }
        });
    }

    #[expect(
        clippy::too_many_lines,
        reason = "splitting the key-dispatch match would hurt readability"
    )]
    async fn input_key(self: Arc<Self>, mut rx: UnboundedReceiver<ExtendedKey>) -> Result<()> {
        let mut sent = false;
        loop {
            tokio::select! {
                Some(key) = rx.recv() => {
                    let mut lock = self.input.write().await;
                    let state = &mut lock as &mut LogState;
                    match key {
                        ExtendedKey::Generic(key) => match key.code {
                            KeyCode::Enter => {
                                send_state(lock);
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
                                    state.out.pos -= 1;
                                    state.out.cursor_to_relative(pos)?;
                                    state.completion.update(&mut state.out, pos);
                                    state.rewrite_input(state.out.length, pos)?;
                                    continue;
                                }
                            }
                            KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {
                                if !state.out.is_at_end() {
                                    if !state.selection.is_active() {
                                        state.selection.start_at(state.out.pos);
                                    }
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
                                    state.out.pos += 1;
                                    state.out.cursor_to_relative(pos)?;
                                    state.completion.update(&mut state.out, pos);
                                    state.rewrite_input(state.out.length, pos)?;
                                    continue;
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
                                    state.out.pos = state.out.length;
                                    let length = state.out.length;
                                    state.completion.update(&mut state.out, length);
                                    state.rewrite_input(length, length)?;
                                    continue;
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
                                    state.out.pos = 0;
                                    state.completion.update(&mut state.out, 0);
                                    state.rewrite_input(state.out.length, 0)?;
                                    continue;
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
                                'j' => {
                                    sent = true;
                                    continue;
                                }
                                _ => continue,
                            }
                        }
                        ExtendedKey::String(mut string) => {
                            if string.contains('\n') {
                                string = string.replace('\n', "");
                                sent = true;
                            }
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

                            if sent {
                                send_state(lock);
                                sent = false;
                            }

                            continue;
                        }
                        ExtendedKey::Resize => {
                            state.rewrite_current_input()?;
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
                    state.out.cursor_to(terminal_width())?;
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

fn send_state(mut lock: RwLockWriteGuard<'_, LogState>) {
    if lock.out.is_empty() || lock.out.text.chars().all(char::is_whitespace) {
        return;
    }
    let message = lock.out.text.clone();
    lock.history.push(message.clone());
    lock.reset().ok();
    drop(lock);
    steel_utils::console!("{}", message);
    if let Some(server) = SERVER.get() {
        server
            .command_dispatcher
            .read()
            .handle_command(CommandSender::Console, message, server);
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
