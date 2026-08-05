use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

use super::{BlockRef, default_surviving_state};

/// Vanilla `MushroomBlock` survival.
// TODO: Implement full vanilla behavior beyond can_survive.
#[block_behavior]
pub struct MushroomBlock {
    block: BlockRef,
}

impl MushroomBlock {
    /// Creates a new mushroom block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for MushroomBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        if below
            .get_block()
            .has_tag(&BlockTag::OVERRIDES_MUSHROOM_LIGHT_REQUIREMENT)
        {
            return true;
        }

        world.raw_brightness(pos, 0) < 13 && below.is_solid_render()
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{REGISTRY, test_support::init_test_registry, vanilla_blocks};

    use crate::test_support::TestLevel;

    use super::*;

    fn single_support_level(support: BlockStateId, raw_brightness: u8) -> TestLevel {
        TestLevel::default()
            .with_block(BlockPos::ZERO.below(), support)
            .with_raw_brightness(raw_brightness)
    }

    #[test]
    fn mushroom_survival_uses_solid_render_support() {
        init_test_registry();

        let mushroom = MushroomBlock::new(&vanilla_blocks::BROWN_MUSHROOM);
        let state = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::BROWN_MUSHROOM);
        let pos = BlockPos::new(0, 0, 0);

        let grass_block = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::GRASS_BLOCK);
        assert!(mushroom.can_survive(state, &single_support_level(grass_block, 12), pos));
        assert!(!mushroom.can_survive(state, &single_support_level(grass_block, 13), pos));

        let oak_leaves = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::OAK_LEAVES);
        assert!(!mushroom.can_survive(state, &single_support_level(oak_leaves, 0), pos));

        let podzol = REGISTRY
            .blocks
            .get_default_state_id(&vanilla_blocks::PODZOL);
        assert!(mushroom.can_survive(state, &single_support_level(podzol, 15), pos));
    }
}
