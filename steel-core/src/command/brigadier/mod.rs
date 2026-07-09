//! Brigadier-compatible command parsing primitives.
//!
//! Semantics are adapted from Brigadier 1.3.10. See `LICENSE-MIT` in this directory.

mod error;
mod reader;
mod string_range;

pub(crate) use error::{CommandSyntaxError, CommandSyntaxErrorKind};
pub(crate) use reader::StringReader;
pub(crate) use string_range::StringRange;

#[cfg(test)]
mod tests;
