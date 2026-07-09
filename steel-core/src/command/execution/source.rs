use std::sync::Arc;

use crate::command::brigadier::CommandSyntaxError;

type CommandResultCallbackFn = dyn Fn(bool, i32) + Send + Sync;

/// A callback invoked after a terminal command returns or fails.
#[derive(Clone, Default)]
pub(crate) struct CommandResultCallback {
    callback: Option<Arc<CommandResultCallbackFn>>,
}

impl CommandResultCallback {
    pub(crate) fn new(callback: impl Fn(bool, i32) + Send + Sync + 'static) -> Self {
        Self {
            callback: Some(Arc::new(callback)),
        }
    }

    pub(crate) const fn empty() -> Self {
        Self { callback: None }
    }

    pub(crate) fn chain(first: Self, second: Self) -> Self {
        match (first.callback, second.callback) {
            (None, None) => Self::empty(),
            (Some(callback), None) | (None, Some(callback)) => Self {
                callback: Some(callback),
            },
            (Some(first), Some(second)) => Self::new(move |success, result| {
                first(success, result);
                second(success, result);
            }),
        }
    }

    pub(crate) fn on_result(&self, success: bool, result: i32) {
        if let Some(callback) = &self.callback {
            callback(success, result);
        }
    }
}

/// Source behavior required by the Steel command scheduler.
pub(crate) trait ExecutionCommandSource: Sized + Send + Sync + 'static {
    fn with_callback(&self, callback: CommandResultCallback) -> Self;

    fn callback(&self) -> CommandResultCallback;

    fn handle_error(&self, error: &CommandSyntaxError, forked: bool);
}
