//! This module contains various codecs for reading and writing data.
/// A module for a bit set.
pub mod bit_set;
/// A module for codec impl for glam crate.
pub mod glam;
/// A module for vanilla `LpVec3` packed vector encoding.
pub mod lp_vec3;
/// A module for an Or type that can be one of two types.
pub mod or;
mod variable_integer;

pub use bit_set::BitSet;
pub use lp_vec3::LpVec3;
pub use or::Or;
pub use variable_integer::{VarInt, VarLong, VarUint};
