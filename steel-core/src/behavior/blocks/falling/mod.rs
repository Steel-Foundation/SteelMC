//! Vanilla falling-block behaviors.

mod concrete_powder_block;
mod falling_block;
mod sand_block;

pub use concrete_powder_block::ConcretePowderBlock;
pub use falling_block::FallingBlock;
pub use sand_block::SandBlock;

#[cfg(test)]
mod tests;
