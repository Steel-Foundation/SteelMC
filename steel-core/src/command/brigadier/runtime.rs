//! Opaque command behavior payloads stored by the Brigadier graph.

use super::{CommandContext, CommandSyntaxError};

/// Selects the executor and redirect-modifier representations stored in a graph.
pub(crate) trait CommandRuntime<S>: 'static {
    type Executor: Send + Sync + ?Sized;
    type Modifier: Send + Sync + ?Sized;
}

/// The standard synchronous behavior used by the standalone Brigadier layer.
pub(crate) struct BrigadierRuntime;

pub(super) type BrigadierExecutor<S> =
    dyn Fn(&CommandContext<S, BrigadierRuntime>) -> Result<i32, CommandSyntaxError> + Send + Sync;
pub(super) type BrigadierModifier<S> = dyn Fn(&CommandContext<S, BrigadierRuntime>) -> Result<Vec<S>, CommandSyntaxError>
    + Send
    + Sync;

impl<S> CommandRuntime<S> for BrigadierRuntime {
    type Executor = BrigadierExecutor<S>;
    type Modifier = BrigadierModifier<S>;
}
