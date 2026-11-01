//! Vanilla falling-block behaviors.

mod concrete_powder_block;
mod dragon_egg_block;
mod falling_block;
mod sand_block;

pub use concrete_powder_block::*;
pub use dragon_egg_block::*;
pub use falling_block::*;
pub use sand_block::*;

#[cfg(test)]
mod tests;
