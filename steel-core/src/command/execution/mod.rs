//! Tick-owned Minecraft command execution.

mod argument;
mod biome;
mod block;
mod coordinates;
mod item;
mod item_predicate;
mod nbt;
mod queue;
mod runtime;
mod score;
mod selector;
mod slot;
mod source;
mod world;

pub(crate) use argument::SteelArgumentType;
pub(crate) use biome::BiomeOrTag;
pub(crate) use block::BlockPredicate;
pub(crate) use coordinates::Coordinates;
pub(crate) use item_predicate::ItemPredicate;
pub(crate) use queue::{ChainModifiers, CommandExecutionContext, ExecutionControl, ExecutionStop};
pub(crate) use runtime::{
    CustomCommandExecutor, CustomModifierExecutor, SteelCommandContext, SteelCommandRuntime,
    SteelContextChain, SteelExecutor, SteelModifier, argument, literal,
};
pub(crate) use score::{IntRange, ScoreHolderArgument, ScoreHolderWildcard};
pub(crate) use slot::ItemSlotRange;
pub(crate) use source::{
    CommandPermissionSource, CommandResultCallback, CommandSource, ExecutionCommandSource,
};
pub(crate) use world::WorldArgument;

#[cfg(test)]
mod argument_tests;
#[cfg(test)]
mod tests;
