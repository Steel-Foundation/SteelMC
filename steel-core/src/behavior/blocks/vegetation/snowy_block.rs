use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::ScheduledTickAccess;

const SNOWY: &BoolProperty = &BlockStateProperties::SNOWY;

/// Vanilla `SnowyBlock` snowy-top behavior.
///
/// Podzol uses this class directly. Grass and mycelium delegate the same logic
/// from their own behavior structs until `SpreadingSnowyBlock` is ported.
#[block_behavior]
pub struct SnowyBlock {
    block: BlockRef,
}

impl SnowyBlock {
    /// Creates a snowy block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns whether the block above should set `snowy=true`.
    #[must_use]
    pub fn snowy_setting(above_state: BlockStateId) -> bool {
        above_state.get_block().has_tag(&BlockTag::SNOW)
    }

    /// Returns the placement state with `snowy` derived from the block above.
    #[must_use]
    pub fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> BlockStateId {
        let above = context.world.get_block_state(context.place_pos().above());
        self.block
            .default_state()
            .set_value(SNOWY, Self::snowy_setting(above))
    }

    /// Recomputes `snowy` when the upward neighbor changes.
    #[must_use]
    pub fn update_shape(
        state: BlockStateId,
        direction: Direction,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Up {
            state.set_value(SNOWY, Self::snowy_setting(neighbor_state))
        } else {
            state
        }
    }
}

impl BlockBehavior for SnowyBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.state_for_placement(context))
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
        Self::update_shape(state, direction, neighbor_state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};
    use steel_registry::init_vanilla_registry;
    use steel_registry::item_stack::ItemStack;
    use steel_registry::vanilla_blocks;
    use steel_registry::vanilla_items;
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::super::{GrassBlock, MyceliumBlock};
    use crate::behavior::context::BlockPlaceContext;

    #[test]
    fn snowy_setting_matches_snow_tag_members() {
        init_vanilla_registry();

        assert!(SnowyBlock::snowy_setting(
            vanilla_blocks::SNOW.default_state()
        ));
        assert!(SnowyBlock::snowy_setting(
            vanilla_blocks::SNOW_BLOCK.default_state()
        ));
        assert!(SnowyBlock::snowy_setting(
            vanilla_blocks::POWDER_SNOW.default_state()
        ));
        assert!(!SnowyBlock::snowy_setting(
            vanilla_blocks::AIR.default_state()
        ));
    }

    #[test]
    fn snowy_block_clears_snowy_on_upward_neighbor_removal() {
        init_vanilla_registry();
        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);
        let state = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, true);

        let updated = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(!updated.get_value(SNOWY));
    }

    #[test]
    fn snowy_block_sets_snowy_on_upward_snow_neighbor() {
        init_vanilla_registry();
        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);
        let state = vanilla_blocks::PODZOL.default_state();

        let updated = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::SNOW.default_state(),
        );

        assert!(updated.get_value(SNOWY));
    }

    #[test]
    fn snowy_block_preserves_snowy_on_non_upward_update() {
        init_vanilla_registry();
        let behavior = SnowyBlock::new(&vanilla_blocks::PODZOL);
        let state = vanilla_blocks::PODZOL
            .default_state()
            .set_value(SNOWY, true);

        let updated = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::DIRT.default_state(),
        );

        assert!(updated.get_value(SNOWY));
    }

    #[test]
    fn grass_block_clears_snowy_on_upward_neighbor_removal() {
        init_vanilla_registry();
        let behavior = GrassBlock::new(&vanilla_blocks::GRASS_BLOCK);
        let state = vanilla_blocks::GRASS_BLOCK
            .default_state()
            .set_value(SNOWY, true);

        let updated = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(!updated.get_value(SNOWY));
    }

    #[test]
    fn mycelium_block_clears_snowy_on_upward_neighbor_removal() {
        init_vanilla_registry();
        let behavior = MyceliumBlock::new(&vanilla_blocks::MYCELIUM);
        let state = vanilla_blocks::MYCELIUM
            .default_state()
            .set_value(SNOWY, true);

        let updated = behavior.update_shape(
            state,
            &TestLevel::default(),
            BlockPos::ZERO,
            Direction::Up,
            BlockPos::ZERO.above(),
            vanilla_blocks::AIR.default_state(),
        );

        assert!(!updated.get_value(SNOWY));
    }

    #[test]
    fn placement_sets_snowy_from_block_above_for_all_snowy_blocks() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("snowy_block_placement");
        insert_ready_full_chunk(&world, ChunkPos::new(0, 0));
        let place_pos = BlockPos::new(8, 64, 8);
        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;

        for (above, expected) in [
            (vanilla_blocks::STONE.default_state(), false),
            (vanilla_blocks::SNOW.default_state(), true),
            (vanilla_blocks::SNOW_BLOCK.default_state(), true),
            (vanilla_blocks::POWDER_SNOW.default_state(), true),
        ] {
            assert!(
                world.set_block_with_limit(place_pos.above(), above, flags, 0),
                "failed to set above block to {above:?}"
            );
            let mut stack = ItemStack::new(&vanilla_items::GRASS_BLOCK);
            let context = BlockPlaceContext::directional(
                &world,
                place_pos,
                Direction::Down,
                &mut stack,
                Direction::Up,
            );

            let podzol = SnowyBlock::new(&vanilla_blocks::PODZOL)
                .get_state_for_placement(&context)
                .expect("podzol should place");
            let grass = GrassBlock::new(&vanilla_blocks::GRASS_BLOCK)
                .get_state_for_placement(&context)
                .expect("grass should place");
            let mycelium = MyceliumBlock::new(&vanilla_blocks::MYCELIUM)
                .get_state_for_placement(&context)
                .expect("mycelium should place");

            assert_eq!(podzol.get_value(SNOWY), expected);
            assert_eq!(grass.get_value(SNOWY), expected);
            assert_eq!(mycelium.get_value(SNOWY), expected);
        }
    }
}
