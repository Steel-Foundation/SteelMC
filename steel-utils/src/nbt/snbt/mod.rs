mod error;
mod number;
mod parser;
mod writer;

pub use error::{SnbtError, SnbtErrorKind, SnbtNumberType};
pub use parser::{
    parse_snbt, parse_snbt_argument, parse_snbt_compound, parse_snbt_compound_argument,
};
pub use writer::to_canonical_snbt;

#[cfg(test)]
mod tests;
