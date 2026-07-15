use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{blocks::BlockRef, sound_events::BLOCK_AMETHYST_BLOCK_CHIME};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext},
    entity::projectile::Projectile,
    world::{ClipHitResult, World},
};

/// Vanilla `AmethystBlock` behavior shared by amethyst blocks and clusters.
#[block_behavior]
pub struct AmethystBlock {
    block: BlockRef,
}

impl AmethystBlock {
    /// Creates an amethyst block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn projectile_hit_pitch(random_fraction: f32) -> f32 {
        0.5 + random_fraction * 1.2
    }

    pub(super) fn play_projectile_hit_sound(world: &World, pos: BlockPos) {
        let pitch = Self::projectile_hit_pitch(rand::random());
        world.play_block_sound(&BLOCK_AMETHYST_BLOCK_CHIME, pos, 1.0, pitch, None);
    }
}

impl BlockBehavior for AmethystBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_projectile_hit(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        hit: &ClipHitResult,
        _projectile: &dyn Projectile,
    ) {
        Self::play_projectile_hit_sound(world, hit.block_pos);
    }
}

#[cfg(test)]
mod tests {
    use super::AmethystBlock;

    #[test]
    fn projectile_chime_pitch_matches_vanilla_range() {
        assert!((AmethystBlock::projectile_hit_pitch(0.0) - 0.5).abs() < f32::EPSILON);
        assert!(AmethystBlock::projectile_hit_pitch(0.999_999) < 1.7);
    }
}
