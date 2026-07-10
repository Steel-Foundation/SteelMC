//! Tick-owned Minecraft command execution.

mod argument;
mod coordinates;
mod item;
mod item_predicate;
mod queue;
mod runtime;
mod selector;
mod source;

pub(crate) use argument::SteelArgumentType;
pub(crate) use coordinates::Coordinates;
pub(crate) use item_predicate::ItemPredicate;
pub(crate) use queue::{ChainModifiers, CommandExecutionContext, ExecutionControl, ExecutionStop};
pub(crate) use runtime::{
    CustomCommandExecutor, CustomModifierExecutor, SteelCommandContext, SteelCommandRuntime,
    SteelContextChain, SteelExecutor, SteelModifier, argument, literal,
};
pub(crate) use source::{
    CommandPermissionSource, CommandResultCallback, CommandSource, ExecutionCommandSource,
};

#[cfg(test)]
mod argument_tests;
#[cfg(test)]
mod tests;
