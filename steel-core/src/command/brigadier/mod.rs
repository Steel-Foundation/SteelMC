//! Brigadier-compatible command parsing primitives.
//!
//! Semantics are adapted from Brigadier 1.3.10. See `LICENSE-MIT` in this directory.

mod argument;
mod builder;
mod context;
mod dispatcher;
mod error;
mod node;
mod reader;
mod string_range;

pub(crate) use argument::ArgumentType;
pub(crate) use builder::{CommandNodeBuilder, argument, literal};
pub(crate) use context::{ParseError, ParseResults, ParsedCommandContext, ParsedCommandNode};
pub(crate) use dispatcher::CommandDispatcher;
pub(crate) use error::{CommandSyntaxError, CommandSyntaxErrorKind};
pub(crate) use node::{
    CommandContext, CommandRequirement, NodeId, NodeKind, RegistrationError, RegistrationErrorKind,
};
pub(crate) use reader::StringReader;
pub(crate) use string_range::StringRange;

#[cfg(test)]
mod argument_tests;
#[cfg(test)]
mod parsing_tests;
#[cfg(test)]
mod registration_tests;
#[cfg(test)]
mod tests;
