//! This module contains various codecs for reading and writing data.
/// A module for a bit set.
pub mod bit_set;
/// A module for an Or type that can be one of two types.
pub mod or;
/// A module for a variable-length integer.
pub mod var_int;
/// A module for a variable-length long integer.
pub mod var_long;
/// A module for a variable-length unsigned integer.
pub mod var_uint;

pub use bit_set::BitSet;
pub use or::Or;
pub use var_int::VarInt;
pub use var_long::VarLong;
pub use var_uint::VarUint;
