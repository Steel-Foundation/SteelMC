use std::{
    fmt::{self, Debug, Display, Formatter, Write},
    sync::{Arc, OnceLock},
};
use tracing::field::{Field, Visit};

/// A reference to the Steel Logger\
/// Use the log macros instead: `log::info`!, `logger::chat`!, etc...
pub static STEEL_LOGGER: OnceLock<Arc<dyn SteelLogger>> = OnceLock::new();

/// Levels of logging in Steel
pub enum Level {
    /// Standard levels from tracing
    Tracing(tracing::Level),
    /// Console input level
    Console,
    /// Chat message level
    #[allow(unused)]
    Chat(String),
    /// Command level: Should contain the executor name
    #[allow(unused)]
    Command(String),
}
impl Display for Level {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Level::Tracing(level) => match *level {
                    tracing::Level::ERROR => "\x1b[0;1;31m[Error]\x1b[0m".to_string(),
                    tracing::Level::WARN => "\x1b[0;1;33m[Warn]\x1b[0m".to_string(),
                    tracing::Level::INFO => "\x1b[0;1;34m[Info]\x1b[0m".to_string(),
                    tracing::Level::DEBUG => "\x1b[0;1;32m[Debug]\x1b[0m".to_string(),
                    tracing::Level::TRACE => "\x1b[0;1;90m[Trace]\x1b[0m".to_string(),
                },
                Level::Console => "\x1b[0;1;35m[Console]\x1b[0m".to_string(),
                Level::Chat(name) => format!("\x1b[0;36m[Chat: {name}]\x1b[0m"),
                Level::Command(name) => format!("\x1b[0;35m[Command: {name}]\x1b[0m"),
            }
        )
    }
}

/// A log macro for console input.
#[macro_export]
macro_rules! console {
    ($($arg:tt)+) =>
        ($crate::logger::STEEL_LOGGER.get().expect("Steel logger isn't initialized!").log(
            $crate::logger::Level::Console,
            $crate::logger::LogData::message(format!($($arg)+)),
        ));
}
/// A log macro for chat messages, provide first the player name, and then the format.
#[macro_export]
macro_rules! chat {
    ($player:expr,$($arg:tt)+) =>
        ($crate::logger::STEEL_LOGGER.get().expect("Steel logger isn't initialized!").log(
            $crate::logger::Level::Chat($player),
            $crate::logger::LogData::message(format!($($arg)+)),
        ));
}
/// A log macro for commands, provide first the player name, and then the format.
#[allow(unused)]
#[macro_export]
macro_rules! command {
    ($player:expr,$($arg:tt)+) =>
        ($crate::logger::STEEL_LOGGER.get().expect("Steel logger isn't initialized!").log(
            $crate::logger::Level::Command($player),
            $crate::logger::LogData::message(format!($($arg)+)),
        ));
}

/// A message visitor for the Steel Logger
#[derive(Default)]
pub struct LogData {
    /// The log message
    pub message: String,
    /// The module path to where logged
    pub module_path: String,
    /// All extra data
    pub extra: String,
}

impl LogData {
    /// Creates a new `LogData`
    #[must_use]
    pub const fn new() -> Self {
        Self {
            message: String::new(),
            module_path: String::new(),
            extra: String::new(),
        }
    }
    /// Creates a `LogData` containing a message
    #[must_use]
    pub fn message(msg: String) -> Self {
        Self {
            message: msg,
            module_path: module_path!().to_string(),
            extra: String::new(),
        }
    }
}

impl Visit for LogData {
    fn record_debug(&mut self, field: &Field, value: &dyn Debug) {
        match field.name() {
            "message" => {
                write!(self.message, "{value:?}").ok();
            }
            "log.module_path" => {
                write!(self.module_path, "{value:?}").ok();
            }
            "log.target" => (),
            name => {
                write!(self.extra, " ({name}: {value:?})").ok();
            }
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => {
                write!(self.message, "{value}").ok();
            }
            "log.module_path" => {
                write!(self.module_path, "{value}").ok();
            }
            "log.target" => (),
            name => {
                write!(self.extra, " ({name}: {value})").ok();
            }
        }
    }
}

/// A trait for Logging Steel data
pub trait SteelLogger: Send + Sync {
    /// Does the logging logic
    fn log(&self, level: Level, data: LogData);
}
