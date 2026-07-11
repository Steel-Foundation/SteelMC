//! Tick-owned Minecraft command execution.

mod argument;
mod biome;
mod block;
mod coordinates;
mod item;
mod item_predicate;
mod nbt;
mod profile;
mod queue;
mod runtime;
mod score;
mod selector;
mod source;
mod structure;
mod text;
mod world;

pub(crate) use argument::SteelArgumentType;
pub(crate) use biome::BiomeOrTag;
pub(crate) use block::BlockPredicate;
pub(crate) use coordinates::Coordinates;
pub(crate) use item_predicate::ItemPredicate;
pub(crate) use profile::GameProfileArgument;
pub(crate) use queue::{
    ChainModifiers, CommandExecutionContext, CommandResultSuspension, CommandResultSuspensionPoll,
    ExecutionControl, ExecutionStop,
};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "custom scheduler extension hooks are retained for keyed command runtimes"
    )
)]
pub(crate) use queue::{CommandSuspension, CommandSuspensionPoll, EntryAction, Frame};
pub(crate) use runtime::{CustomCommandExecutor, CustomModifierExecutor};
pub(crate) use runtime::{
    SteelCommandContext, SteelCommandRuntime, SteelContextChain, SteelExecutor, SteelModifier,
    argument, literal,
};
pub(crate) use score::{IntRange, ScoreHolderArgument, ScoreHolderWildcard};
pub(crate) use source::{
    CommandArgumentSource, CommandPermissionSource, CommandResultCallback, CommandSource,
    ExecutionCommandSource,
};
pub(crate) use structure::StructureOrTagKey;
pub(crate) use text::CommandTextResolver;
pub(crate) use world::WorldArgument;

#[cfg(test)]
mod argument_tests;
#[cfg(test)]
mod tests;
