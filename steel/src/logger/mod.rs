use crate::config::{LogConfig, LogTimeFormat};
use chrono::Utc;
use crossterm::{
    style::{Color::DarkGrey, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, disable_raw_mode},
};
use std::{
    io::Write,
    sync::Arc,
    time::{self, Instant},
};
use steel_utils::locks::AsyncRwLock;
use steel_utils::logger::{Level, LogData, STEEL_LOGGER, SteelLogger};
use tokio::{sync::mpsc, task, time::timeout};
use tokio_util::sync::CancellationToken;
use tracing::Subscriber;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

mod file;
mod history;
mod input;
mod output;
mod selection;
mod state;
mod suggestions;

/// Returns the terminal width, falling back to 80 columns if unavailable or it's <= 0.
fn terminal_width() -> usize {
    terminal::size().map_or(80, |(w, _)| if w == 0 { 80 } else { w as usize })
}
/// Returns the terminal height, falling back to 30 rows if unavailable or 1 if it's <= 0.
fn terminal_height() -> usize {
    terminal::size().map_or(30, |(_, h)| if h == 0 { 30 } else { h as usize })
}

pub(crate) use state::LogState;

pub(crate) enum Move {
    None,
    Up,
    Down,
}

/// A logger implementation with commands suggestions
pub struct CommandLogger {
    input: Arc<AsyncRwLock<LogState>>,
    sender: mpsc::UnboundedSender<(Level, LogData)>,
    cancel_token: CancellationToken,
    stopped: CancellationToken,
    log_stopped: CancellationToken,
    start_time: Instant,
    log_config: Option<LogConfig>,
}

impl CommandLogger {
    /// Initializes the `CommandLogger`
    pub async fn init(
        cancel_token: CancellationToken,
        log_config: Option<LogConfig>,
    ) -> Result<Arc<Self>, String> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let log_cancel_token = CancellationToken::new();
        let input = LogState::new(log_config.as_ref(), cancel_token)
            .await
            .map_err(|err| format!("failed to initialize logger state: {err}"))?;

        let log = Arc::new(Self {
            input: Arc::new(AsyncRwLock::const_new(input)),
            sender,
            cancel_token: log_cancel_token.clone(),
            stopped: CancellationToken::new(),
            log_stopped: CancellationToken::new(),
            start_time: Instant::now(),
            log_config,
        });
        task::spawn(log.clone().log_loop(receiver));
        task::spawn(log.clone().input_main());
        STEEL_LOGGER
            .set(log.clone())
            .map_err(|_| "Steel logger is already initialized".to_string())?;
        Ok(log)
    }

    /// Stops the logger and waits for cleanup to complete
    pub async fn stop(&self) {
        self.cancel_token.cancel();
        if timeout(time::Duration::from_secs(1), self.stopped.cancelled())
            .await
            .is_err()
        {
            let _ = disable_raw_mode();
            self.stopped.cancel();
        }
        if timeout(time::Duration::from_secs(1), self.log_stopped.cancelled())
            .await
            .is_err()
        {
            eprintln!("Timed out waiting for logger to flush pending entries");
            self.log_stopped.cancel();
        }
    }

    async fn log_loop(self: Arc<Self>, mut receiver: mpsc::UnboundedReceiver<(Level, LogData)>) {
        loop {
            tokio::select! {
                biased;
                Some((lvl, data)) = receiver.recv() => {
                    self.write_entry(lvl, data).await;
                }
                () = self.cancel_token.cancelled() => {
                    while let Ok((lvl, data)) = receiver.try_recv() {
                        self.write_entry(lvl, data).await;
                    }
                    self.flush_file().await;
                    self.log_stopped.cancel();
                    break;
                }
            }
        }
    }

    async fn write_entry(&self, lvl: Level, data: LogData) {
        let (lvl, data) = self.write_log_entry(lvl, data).await;
        if self.log_config.as_ref().is_some_and(|l| l.log_file) {
            self.write_file_entry(lvl, data).await;
        }
    }

    async fn write_log_entry(&self, lvl: Level, data: LogData) -> (Level, LogData) {
        let mut input = self.input.write().await;

        if let Err(err) = input.out.cursor_to(0) {
            log::error!("{err}");
            return (lvl, data);
        }

        let time_str = self.format_time();
        let module_path_str = self.format_module_path(&data, true);
        let extra_str = self.format_extra(&data, true);
        let rendered = normalize_terminal_newlines(&format!(
            "{time_str}{lvl} {module_path_str}{}{extra_str}",
            data.message
        ));
        let consumed_rows = rendered_terminal_rows(&rendered, terminal_width());

        if let Err(err) = writeln!(
            input.out,
            "{}{rendered}\r",
            Clear(ClearType::FromCursorDown),
        ) {
            log::error!("{err}");
            return (lvl, data);
        }

        input.completion.consume_reserved_rows(consumed_rows);

        let pos = input.out.pos;
        if let Err(err) = input.out.cursor_to_relative(pos) {
            log::error!("{err}");
        }
        if let Err(err) = input.rewrite_current_input() {
            log::error!("{err}");
        }
        (lvl, data)
    }

    async fn write_file_entry(&self, lvl: Level, data: LogData) {
        let mut input = self.input.write().await;

        let time_str = self.format_time();
        let module_path_str = self.format_module_path(&data, false);
        let extra_str = self.format_extra(&data, false);

        if let Err(err) = writeln!(
            input.file,
            "{time_str}{lvl:?} {module_path_str}{}{extra_str}",
            strip_ansi_escapes::strip_str(&data.message),
        ) {
            input.file.disable();
            eprintln!("Failed to write log file; disabling file logging: {err}");
        }
    }

    async fn flush_file(&self) {
        let mut input = self.input.write().await;
        if let Err(err) = input.file.flush() {
            eprintln!("Failed to flush log file: {err}");
        }
    }

    fn format_time(&self) -> String {
        match self.log_config.as_ref().map(|l| &l.time) {
            Some(LogTimeFormat::Date) => {
                let time: chrono::DateTime<Utc> = time::SystemTime::now().into();
                format!("{} ", time.format("%T:%3f"))
            }
            Some(LogTimeFormat::Uptime) => {
                let elapsed = self.start_time.elapsed();
                format!("{:>6.2}s ", elapsed.as_secs_f64())
            }
            _ => String::new(),
        }
    }

    fn format_module_path(&self, data: &LogData, color: bool) -> String {
        if self.log_config.as_ref().is_some_and(|l| l.module_path) {
            if color {
                format!(
                    " {}{}{} ",
                    SetForegroundColor(DarkGrey),
                    data.module_path,
                    ResetColor
                )
            } else {
                format!(" {} ", data.module_path)
            }
        } else {
            String::new()
        }
    }

    fn format_extra(&self, data: &LogData, color: bool) -> String {
        if self.log_config.as_ref().is_some_and(|l| l.extra) {
            if color {
                format!(
                    "{}{}{}",
                    SetForegroundColor(DarkGrey),
                    data.extra,
                    ResetColor
                )
            } else {
                data.extra.clone()
            }
        } else {
            String::new()
        }
    }
}

fn rendered_terminal_rows(rendered: &str, terminal_width: usize) -> usize {
    let terminal_width = terminal_width.max(1);
    let rendered = rendered.replace('\t', "        ");
    strip_ansi_escapes::strip_str(&rendered)
        .split('\n')
        .map(|line| {
            let columns = line
                .bytes()
                .map(|byte| usize::from(byte != b'\r'))
                .sum::<usize>();
            columns.div_ceil(terminal_width).max(1)
        })
        .sum()
}

fn normalize_terminal_newlines(rendered: &str) -> String {
    rendered.replace("\r\n", "\n").replace('\n', "\r\n")
}

impl SteelLogger for CommandLogger {
    fn log(&self, lvl: Level, data: LogData) {
        self.sender.send((lvl, data)).ok();
    }
}

/// A logger layer for tracing
pub struct LoggerLayer(pub Arc<CommandLogger>);

impl LoggerLayer {
    /// Creates a new logger
    pub async fn new(
        cancel_token: CancellationToken,
        log_config: Option<LogConfig>,
    ) -> Result<Self, String> {
        Ok(Self(CommandLogger::init(cancel_token, log_config).await?))
    }
}

impl<S: Subscriber> Layer<S> for LoggerLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut data = LogData::new();
        event.record(&mut data);
        self.0.log(Level::Tracing(*event.metadata().level()), data);
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_terminal_newlines, rendered_terminal_rows};

    #[test]
    fn rendered_rows_include_wrapping_and_trailing_newlines() {
        assert_eq!(rendered_terminal_rows("short", 10), 1);
        assert_eq!(rendered_terminal_rows("12345678901", 10), 2);
        assert_eq!(rendered_terminal_rows("line\n", 10), 2);
        assert_eq!(rendered_terminal_rows("first\nsecond", 10), 2);
    }

    #[test]
    fn rendered_rows_ignore_ansi_and_count_tabs_conservatively() {
        assert_eq!(rendered_terminal_rows("\u{1b}[31mshort\u{1b}[0m", 10), 1);
        assert_eq!(rendered_terminal_rows("\t", 4), 2);
    }

    #[test]
    fn terminal_newlines_reset_the_output_column() {
        assert_eq!(
            normalize_terminal_newlines("first\nsecond"),
            "first\r\nsecond"
        );
        assert_eq!(
            normalize_terminal_newlines("first\r\nsecond"),
            "first\r\nsecond"
        );
        assert_eq!(rendered_terminal_rows("12345\r\n123456", 10), 2);
    }
}
