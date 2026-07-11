//! Brigadier-compatible command parsing primitives.
//!
//! Semantics are adapted from Brigadier 1.3.10. See `LICENSE-MIT` in this directory.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the complete Brigadier foundation includes primitives not used by current built-ins"
    )
)]

mod argument;
mod builder;
mod context;
mod context_chain;
mod dispatcher;
mod error;
mod node;
mod reader;
mod runtime;
mod string_range;
mod suggestion;

pub(crate) use argument::{
    ArgumentType, CommandArgumentParser, ContainsPrimitiveArgumentValue, PrimitiveArgumentValue,
    StringType,
};
pub(crate) use builder::{CommandNodeBuilder, CommandRequirementRoute};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "generic Brigadier builders remain available for future internal command runtimes"
    )
)]
pub(crate) use builder::{argument, literal};
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "parsed node metadata is part of the tested Brigadier foundation"
    )
)]
pub(crate) use context::ParsedCommandNode;
pub(crate) use context::{
    ArgumentSuggestionContext, CommandContext, ParseError, ParseResults, ParsedCommandContext,
};
pub(crate) use context_chain::{ContextChain, ContextChainStage};
pub(crate) use dispatcher::CommandDispatcher;
pub(crate) use error::{CommandSyntaxError, CommandSyntaxErrorKind};
pub(crate) use node::{
    CommandRedirectTarget, CommandRequirement, NodeId, NodeKind, RegistrationError,
    RegistrationErrorKind,
};
pub(crate) use reader::{ReaderCursor, StringReader};
pub(crate) use runtime::{BrigadierRuntime, CommandRuntime};
pub(crate) use string_range::StringRange;
#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "direct suggestion construction is part of the tested Brigadier foundation"
    )
)]
pub(crate) use suggestion::Suggestion;
pub(crate) use suggestion::{SuggestionError, Suggestions, SuggestionsBuilder};

#[cfg(test)]
mod argument_tests;
#[cfg(test)]
mod context_chain_tests;
#[cfg(test)]
mod parsing_tests;
#[cfg(test)]
mod registration_tests;
#[cfg(test)]
mod runtime_model_tests;
#[cfg(test)]
mod suggestion_tests;
#[cfg(test)]
mod tests;
