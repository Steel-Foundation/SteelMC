use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::ScheduledTickAccess;

#[must_use]
pub(super) fn is_snowy_setting(above_state: BlockStateId) -> bool {
    above_state.get_block().has_tag(&BlockTag::SNOW)
}

#[must_use]
pub(super) fn snowy_placement_state(
    block: BlockRef,
    context: &BlockPlaceContext<'_>,
) -> BlockStateId {
    let above = context.world.get_block_state(context.place_pos().above());
    block
        .default_state()
        .set_value(&BlockStateProperties::SNOWY, is_snowy_setting(above))
}

#[must_use]
pub(super) fn update_snowy_shape(
    state: BlockStateId,
    direction: Direction,
    neighbor_state: BlockStateId,
) -> BlockStateId {
    if direction == Direction::Up {
        state.set_value(
            &BlockStateProperties::SNOWY,
            is_snowy_setting(neighbor_state),
        )
    } else {
        state
    }
}

/// Behavior for podzol and other basic snowy dirt blocks.
#[block_behavior]
pub struct SnowyBlock {
    block: BlockRef,
}

impl SnowyBlock {
    /// Creates a new snowy block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SnowyBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(snowy_placement_state(self.block, context))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        update_snowy_shape(state, direction, neighbor_state)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
    use steel_registry::item_stack::ItemStack;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
    use steel_utils::types::UpdateFlags;
    use steel_utils::{BlockPos, ChunkPos, Direction};

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};

    const SNOWY: &BoolProperty = &BlockStateProperties::SNOWY;

    #[test]
    fn placement_resolves_snowy_from_above_block() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("snowy_block_placement");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));

        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);

        for snow in [
            &vanilla_blocks::SNOW,
            &vanilla_blocks::SNOW_BLOCK,
            &vanilla_blocks::POWDER_SNOW,
        ] {
            world.set_block(pos.above(), snow.default_state(), UpdateFlags::empty());
            let mut stack = ItemStack::empty();
            let context = BlockPlaceContext::directional(
                &world,
                pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            let state = behavior
                .get_state_for_placement(&context)
                .expect("placement state should exist");
            assert!(state.get_value(SNOWY));
        }

        for non_snow in [
            &vanilla_blocks::AIR,
            &vanilla_blocks::STONE,
            &vanilla_blocks::ICE,
        ] {
            world.set_block(pos.above(), non_snow.default_state(), UpdateFlags::empty());
            let mut stack = ItemStack::empty();
            let context = BlockPlaceContext::directional(
                &world,
                pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );
            let state = behavior
                .get_state_for_placement(&context)
                .expect("placement state should exist");
            assert!(!state.get_value(SNOWY));
        }
    }

    #[test]
    fn upward_neighbor_update_recalculates_snowy() {
        init_vanilla_registry();
        init_behaviors();

        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);
        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);

        let base = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, false);
        let snowy = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, true);

        for snow in [
            &vanilla_blocks::SNOW,
            &vanilla_blocks::SNOW_BLOCK,
            &vanilla_blocks::POWDER_SNOW,
        ] {
            let state = behavior.update_shape(
                base,
                &level,
                pos,
                Direction::Up,
                pos.above(),
                snow.default_state(),
            );
            assert!(state.get_value(SNOWY));
        }

        for non_snow in [
            &vanilla_blocks::AIR,
            &vanilla_blocks::STONE,
            &vanilla_blocks::ICE,
        ] {
            let state = behavior.update_shape(
                snowy,
                &level,
                pos,
                Direction::Up,
                pos.above(),
                non_snow.default_state(),
            );
            assert!(!state.get_value(SNOWY));
        }
    }

    #[test]
    fn lateral_and_downward_neighbor_updates_keep_existing_state() {
        init_vanilla_registry();
        init_behaviors();

        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);
        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);

        let base = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, false);
        let snowy = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, true);

        for dir in [
            Direction::Down,
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let neighbor_pos = dir.relative(pos);

            let updated_snowy = behavior.update_shape(
                snowy,
                &level,
                pos,
                dir,
                neighbor_pos,
                vanilla_blocks::AIR.default_state(),
            );
            assert_eq!(updated_snowy, snowy);

            let updated_base = behavior.update_shape(
                base,
                &level,
                pos,
                dir,
                neighbor_pos,
                vanilla_blocks::SNOW.default_state(),
            );
            assert_eq!(updated_base, base);
        }
    }
}
