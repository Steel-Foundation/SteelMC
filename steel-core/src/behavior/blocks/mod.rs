//! Block behavior implementations for vanilla blocks.
//!
//! The actual behavior registration is auto-generated from classes.json.
//! See `src/generated/behaviors.rs` for the generated registration code.

mod building;
mod colored;
mod container;
mod decoration;
mod face_attached_horizontal_directional_block;
mod falling;
mod fluid;
mod portal;
mod redstone;
mod utils;
pub mod vegetation;

pub use building::*;
pub use colored::*;
pub use container::*;
pub use decoration::*;
pub use falling::*;
pub use fluid::*;
pub use portal::*;
pub use redstone::*;
pub(crate) use utils::multiface_face_property;
pub use vegetation::*;
