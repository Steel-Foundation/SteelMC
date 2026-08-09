use steel_macros::block_behavior;
use steel_registry::{
    blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties},
    item_stack::ItemStack,
    items::ItemRef,
};
use steel_utils::{BlockPos, BlockStateId, Direction, Identifier};

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation, default_surviving_state,
            vegetation_block::{survival_update_shape, vegetation_can_survive},
        },
    },
    world::{LevelReader, ScheduledTickAccess},
};

/// Vanilla attached pumpkin and melon stem behavior.
#[block_behavior]
pub struct AttachedStemBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    stem: BlockRef,
    #[json_arg(vanilla_blocks)]
    fruit: BlockRef,
    #[json_arg(vanilla_items)]
    seed: ItemRef,
    #[json_arg(vanilla_block_tags)]
    support_blocks: Identifier,
}

impl AttachedStemBlock {
    /// Creates an attached stem with its extracted stem, fruit, seed, and support tag.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        stem: BlockRef,
        fruit: BlockRef,
        seed: ItemRef,
        support_blocks: Identifier,
    ) -> Self {
        Self {
            block,
            stem,
            fruit,
            seed,
            support_blocks,
        }
    }
}

impl BlockBehavior for AttachedStemBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }

    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
            && neighbor_state.get_block() != self.fruit
        {
            return self
                .stem
                .default_state()
                .set_value(&BlockStateProperties::AGE_7, 7);
        }

        survival_update_shape(self, state, world, pos)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(self.seed))
    }
}

impl Vegetation for AttachedStemBlock {
    fn may_place_on(&self, state: BlockStateId, _world: &dyn LevelReader, _pos: BlockPos) -> bool {
        state.get_block().has_tag(&self.support_blocks)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        init_vanilla_registry, vanilla_block_tags::BlockTag, vanilla_blocks, vanilla_items,
    };

    use crate::{
        behavior::{BLOCK_BEHAVIORS, init_behaviors},
        test_support::TestLevel,
    };

    use super::*;

    fn pumpkin_attached() -> AttachedStemBlock {
        AttachedStemBlock::new(
            &vanilla_blocks::ATTACHED_PUMPKIN_STEM,
            &vanilla_blocks::PUMPKIN_STEM,
            &vanilla_blocks::PUMPKIN,
            &vanilla_items::PUMPKIN_SEEDS,
            BlockTag::SUPPORTS_PUMPKIN_STEM,
        )
    }

    #[test]
    fn attached_stem_keeps_its_fruit_and_reverts_to_mature_stem() {
        init_vanilla_registry();
        let behavior = pumpkin_attached();
        let pos = BlockPos::ZERO;
        let fruit_pos = pos.north();
        let level = TestLevel::default()
            .with_block(pos.below(), vanilla_blocks::FARMLAND.default_state())
            .with_block(fruit_pos, vanilla_blocks::PUMPKIN.default_state());
        let attached = vanilla_blocks::ATTACHED_PUMPKIN_STEM.default_state();

        assert_eq!(
            behavior.update_shape(
                attached,
                &level,
                pos,
                Direction::North,
                fruit_pos,
                vanilla_blocks::PUMPKIN.default_state(),
            ),
            attached
        );

        let reverted = behavior.update_shape(
            attached,
            &level,
            pos,
            Direction::North,
            fruit_pos,
            vanilla_blocks::AIR.default_state(),
        );
        assert_eq!(reverted.get_block(), &vanilla_blocks::PUMPKIN_STEM);
        assert_eq!(reverted.get_value(&BlockStateProperties::AGE_7), 7);
    }

    #[test]
    fn attached_stem_breaks_when_its_support_disappears() {
        init_vanilla_registry();
        let behavior = pumpkin_attached();
        let attached = vanilla_blocks::ATTACHED_PUMPKIN_STEM.default_state();
        let unsupported = TestLevel::default();

        let updated = behavior.update_shape(
            attached,
            &unsupported,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::AIR.default_state(),
        );
        assert!(updated.is_air());
    }

    #[test]
    fn registered_attached_mappings_are_not_swappable() {
        init_vanilla_registry();
        init_behaviors();
        let cases = [
            (
                &vanilla_blocks::ATTACHED_PUMPKIN_STEM,
                &vanilla_blocks::PUMPKIN,
                &vanilla_blocks::MELON,
                &vanilla_blocks::PUMPKIN_STEM,
                &*vanilla_items::PUMPKIN_SEEDS,
            ),
            (
                &vanilla_blocks::ATTACHED_MELON_STEM,
                &vanilla_blocks::MELON,
                &vanilla_blocks::PUMPKIN,
                &vanilla_blocks::MELON_STEM,
                &*vanilla_items::MELON_SEEDS,
            ),
        ];

        for (attached_block, fruit, wrong_fruit, stem, seed) in cases {
            let behavior = BLOCK_BEHAVIORS.get_behavior(attached_block);
            let pos = BlockPos::ZERO;
            let fruit_pos = pos.north();
            let level = TestLevel::default()
                .with_block(pos.below(), vanilla_blocks::FARMLAND.default_state())
                .with_block(fruit_pos, fruit.default_state());
            let attached = attached_block.default_state();

            assert_eq!(
                behavior.update_shape(
                    attached,
                    &level,
                    pos,
                    Direction::North,
                    fruit_pos,
                    fruit.default_state(),
                ),
                attached
            );
            let reverted = behavior.update_shape(
                attached,
                &level,
                pos,
                Direction::North,
                fruit_pos,
                wrong_fruit.default_state(),
            );
            assert_eq!(reverted.get_block(), stem);
            assert_eq!(reverted.get_value(&BlockStateProperties::AGE_7), 7);

            let clone = behavior
                .get_clone_item_stack(attached_block, attached, false)
                .expect("attached stem has a clone item");
            assert_eq!(clone.item(), seed);
        }
    }
}
