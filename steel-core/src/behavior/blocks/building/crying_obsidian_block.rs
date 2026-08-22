//! `CryingObsidianBlock` behavior (`net.minecraft.world.level.block.CryingObsidianBlock`).
//!
//! Vanilla `CryingObsidianBlock` only overrides `animateTick` to spawn
//! `DRIPPING_OBSIDIAN_TEAR` particles (1/5 chance, random face, offset
//! `0.5 + dir*0.6` or `random()`). In vanilla this is `Level.addParticle` —
//! client-local, so Steel has no server-side work (see `AGENTS.md` particle
//! routing). Light emission (level 10) comes from registry block properties,
//! not the behavior.

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};

/// Vanilla `CryingObsidianBlock`.
#[block_behavior]
pub struct CryingObsidianBlock {
    block: BlockRef,
}

impl CryingObsidianBlock {
    /// Creates the behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CryingObsidianBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }
}
