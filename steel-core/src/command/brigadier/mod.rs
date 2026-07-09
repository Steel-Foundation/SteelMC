//! Brigadier-compatible command parsing primitives.
//!
//! Semantics are adapted from Brigadier 1.3.10. See `LICENSE-MIT` in this directory.

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

pub(crate) use argument::{ArgumentType, StringType};
pub(crate) use builder::{CommandNodeBuilder, argument, literal};
pub(crate) use context::{
    CommandContext, ParseError, ParseResults, ParsedCommandContext, ParsedCommandNode,
};
pub(crate) use context_chain::{ContextChain, ContextChainStage};
pub(crate) use dispatcher::CommandDispatcher;
pub(crate) use error::{CommandSyntaxError, CommandSyntaxErrorKind};
pub(crate) use node::{
    CommandRequirement, NodeId, NodeKind, RegistrationError, RegistrationErrorKind,
};
pub(crate) use reader::StringReader;
pub(crate) use runtime::{BrigadierRuntime, CommandRuntime};
pub(crate) use string_range::StringRange;
pub(crate) use suggestion::{Suggestion, SuggestionError, Suggestions, SuggestionsBuilder};

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
