//! Opaque command behavior payloads stored by the Brigadier graph.

use super::{
    ArgumentType, CommandArgumentParser, CommandContext, CommandSyntaxError, PrimitiveArgumentValue,
};

/// Selects the executor and redirect-modifier representations stored in a graph.
pub(crate) trait CommandRuntime<S>: 'static {
    type Argument: CommandArgumentParser<S, Value = Self::ArgumentValue>;
    type ArgumentValue: Clone + Send + Sync + 'static;
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
    type Argument = ArgumentType;
    type ArgumentValue = PrimitiveArgumentValue;
    type Executor = BrigadierExecutor<S>;
    type Modifier = BrigadierModifier<S>;
}
