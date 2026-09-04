//! Specialized collision context for minecarts.

use crate::behavior::BlockCollisionContext;

/// A collision context used during minecart movement resolution.
/// Matches vanilla `MinecartCollisionContext`.
pub struct MinecartCollisionContext {
    base: BlockCollisionContext,
}

impl MinecartCollisionContext {
    /// Creates a new collision context for a minecart.
    #[must_use]
    pub const fn new(_descending: bool) -> Self {
        Self {
            base: BlockCollisionContext::empty(),
        }
    }

    /// Returns the underlying block collision context.
    #[must_use]
    pub const fn as_block_context(&self) -> BlockCollisionContext {
        self.base
    }
}
