//! Server log output tagged with the calling extension's name.
//!
//! [`export_extension!`](crate::export_extension) records the name right after binding the host API,
//! before `pre_init` runs. Before that (for example in a extension's unit tests) messages go to
//! standard error instead of the unbound host logger.

use crate::LogLevel;
use std::sync::OnceLock;

static EXTENSION_NAME: OnceLock<&'static str> = OnceLock::new();

#[doc(hidden)]
pub fn set_extension_name(name: &'static str) {
    let _ = EXTENSION_NAME.set(name);
}

fn write(level: LogLevel, message: &str) {
    let Some(name) = EXTENSION_NAME.get().copied() else {
        eprintln!("[{level:?}] {message}");
        return;
    };
    crate::log_message(name.into(), level, message.into());
}

/// Logs an error.
pub fn error(message: impl AsRef<str>) {
    write(LogLevel::Error, message.as_ref());
}

/// Logs a warning.
pub fn warn(message: impl AsRef<str>) {
    write(LogLevel::Warn, message.as_ref());
}

/// Logs normal operation.
pub fn info(message: impl AsRef<str>) {
    write(LogLevel::Info, message.as_ref());
}

/// Logs diagnostic detail.
pub fn debug(message: impl AsRef<str>) {
    write(LogLevel::Debug, message.as_ref());
}
