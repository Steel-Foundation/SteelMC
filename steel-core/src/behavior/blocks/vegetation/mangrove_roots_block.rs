use steel_macros::block_behavior;
use steel_registry::blocks::properties::Direction;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::WaterloggedTransparentBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::world::ScheduledTickAccess;

use super::BlockRef;

/// Vanilla mangrove roots behavior.
#[block_behavior]
pub struct MangroveRootsBlock {
    waterlogged: WaterloggedTransparentBlock,
}

impl MangroveRootsBlock {
    /// Creates a new mangrove roots behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            waterlogged: WaterloggedTransparentBlock::new(block),
        }
    }
}

impl BlockBehavior for MangroveRootsBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.waterlogged.get_state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.waterlogged
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_fluids, vanilla_items};
    use steel_utils::{ChunkPos, types::UpdateFlags};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;

    #[test]
    fn placement_preserves_source_water() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("mangrove_roots_placement");
        let wet_pos = BlockPos::new(8, 64, 8);
        let dry_pos = wet_pos.east();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(wet_pos));
        assert!(world.set_block(
            wet_pos,
            vanilla_blocks::WATER.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let behavior = MangroveRootsBlock::new(&vanilla_blocks::MANGROVE_ROOTS);

        let wet_state = {
            let mut stack = ItemStack::new(&vanilla_items::MANGROVE_ROOTS);
            let context = BlockPlaceContext::directional(
                &world,
                wet_pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            behavior
                .get_state_for_placement(&context)
                .expect("mangrove roots should have a wet placement state")
        };
        let dry_state = {
            let mut stack = ItemStack::new(&vanilla_items::MANGROVE_ROOTS);
            let context = BlockPlaceContext::directional(
                &world,
                dry_pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            behavior
                .get_state_for_placement(&context)
                .expect("mangrove roots should have a dry placement state")
        };

        assert!(wet_state.get_value(WATERLOGGED));
        assert!(!dry_state.get_value(WATERLOGGED));
    }

    #[test]
    fn waterlogged_neighbor_update_schedules_water_tick() {
        init_vanilla_registry();
        let behavior = MangroveRootsBlock::new(&vanilla_blocks::MANGROVE_ROOTS);
        let wet_state = vanilla_blocks::MANGROVE_ROOTS
            .default_state()
            .set_value(WATERLOGGED, true);
        let dry_state = wet_state.set_value(WATERLOGGED, false);
        let wet_level = TestLevel::default();
        let dry_level = TestLevel::default();

        assert_eq!(
            behavior.update_shape(
                wet_state,
                &wet_level,
                BlockPos::ZERO,
                Direction::Up,
                BlockPos::ZERO.above(),
                vanilla_blocks::AIR.default_state(),
            ),
            wet_state
        );
        assert_eq!(
            behavior.update_shape(
                dry_state,
                &dry_level,
                BlockPos::ZERO,
                Direction::Up,
                BlockPos::ZERO.above(),
                vanilla_blocks::AIR.default_state(),
            ),
            dry_state
        );
        assert!(wet_level.scheduled_water_tick());
        assert!(!dry_level.scheduled_water_tick());
    }

    #[test]
    fn waterlogged_roots_contain_source_water() {
        init_vanilla_registry();
        let state = vanilla_blocks::MANGROVE_ROOTS
            .default_state()
            .set_value(WATERLOGGED, true);

        let fluid = state.get_fluid_state();

        assert_eq!(fluid.fluid_id, &vanilla_fluids::WATER);
        assert!(fluid.is_source());
    }
}
