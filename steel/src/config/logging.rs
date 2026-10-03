use serde::Deserialize;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::filter::Directive;

fn default_log_path() -> String {
    "./.logs".to_string()
}

const fn default_log_file() -> bool {
    true
}

const fn default_max_history() -> usize {
    LogConfig::DEFAULT_MAX_HISTORY
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogConfig {
    /// Path where store the log files and history
    #[serde(default = "default_log_path")]
    pub log_path: String,
    /// The level of information the logger will show
    #[serde(default)]
    pub log_level: LogLevel,
    /// Time display format: "none", "date" (HH:MM:SS:mmm), or "uptime" (seconds since start)
    #[serde(default)]
    pub time: LogTimeFormat,
    /// Whether the `module_path` of the log should be displayed
    #[serde(default)]
    pub module_path: bool,
    /// Whether the extra data of the log should be displayed
    #[serde(default)]
    pub extra: bool,
    /// Whether the log should be written into a file
    #[serde(default = "default_log_file")]
    pub log_file: bool,
    /// Time between log file rotations
    #[serde(default)]
    pub rotation_time: RotationTimeFormat,
    /// Amount of console commands saved
    #[serde(default = "default_max_history")]
    pub max_history: usize,
}

impl LogConfig {
    /// Default console command history length when logging config is absent.
    pub const DEFAULT_MAX_HISTORY: usize = 50;
}

/// Time format for log entries
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogTimeFormat {
    /// No time displayed
    None,
    /// Current time (HH:MM:SS:mmm)
    #[default]
    Date,
    /// Seconds since server start
    Uptime,
}

/// Time for log files rotation
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RotationTimeFormat {
    /// No rotation
    None,
    /// Rotate hourly
    Hourly,
    /// Rotate daily
    #[default]
    Daily,
    /// Rotate weekly
    Weekly,
    /// Rotate monthly
    Monthly,
}

/// The level of information the logger will show
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Only error logs
    Error,
    /// Error and warn logs
    Warn,
    /// All standard logs
    #[default]
    Info,
    /// Standard + Debug info enabled
    Debug,
    /// All logs are shown
    Trace,
}

impl LogLevel {
    /// Converts the log level in it's respective logging directive
    #[must_use]
    pub fn to_directive(self) -> Directive {
        match self {
            LogLevel::Error => LevelFilter::ERROR.into(),
            LogLevel::Warn => LevelFilter::WARN.into(),
            LogLevel::Info => LevelFilter::INFO.into(),
            LogLevel::Debug => LevelFilter::DEBUG.into(),
            LogLevel::Trace => LevelFilter::TRACE.into(),
        }
    }
}
