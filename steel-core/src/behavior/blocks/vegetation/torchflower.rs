use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt,
        properties::{BlockStateProperties, IntProperty},
    },
    item_stack::ItemStack,
    vanilla_blocks, vanilla_items,
};
use steel_utils::BlockStateId;

use crate::{
    behavior::blocks::vegetation::{
        bonemealable::{Bonemealable, CropBonemealExt},
        crop_block::CropLike,
    },
    world::{LevelReader, World},
};

/// Behavior for the Torchflower Block
#[block_behavior]
pub struct TorchflowerCropBlock {
    block: BlockRef,
}

impl TorchflowerCropBlock {
    /// Creates a new crop block behavior with a custom age property.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl CropLike for TorchflowerCropBlock {
    fn block(&self) -> BlockRef {
        self.block
    }

    fn age_property(&self) -> &IntProperty {
        &BlockStateProperties::AGE_1
    }

    fn max_age(&self) -> u8 {
        2
    }

    fn get_state_for_age(&self, age: u8) -> BlockStateId {
        if age == 2 {
            vanilla_blocks::TORCHFLOWER.default_state()
        } else {
            self.block
                .default_state()
                .set_value(self.age_property(), age)
        }
    }

    fn clone_item_stack(&self) -> ItemStack {
        ItemStack::new(&vanilla_items::TORCHFLOWER_SEEDS)
    }

    fn should_random_tick(&self) -> bool {
        rand::random_range(0..3) != 0
    }
}

impl Bonemealable for TorchflowerCropBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, _rng: &mut dyn rand::Rng) -> u8 {
        1
    }

    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: steel_utils::BlockPos,
    ) -> bool {
        !self.is_max_age(state)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn rand::Rng,
        pos: steel_utils::BlockPos,
    ) {
        self.default_perform_bonemeal(state, world, rng, pos);
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    #[test]
    fn torchflower_age_two_becomes_flower_block() {
        init_test_registry();
        let behavior = TorchflowerCropBlock::new(&vanilla_blocks::TORCHFLOWER_CROP);

        let age_one = behavior.get_state_for_age(1);
        assert_eq!(age_one.get_block(), &vanilla_blocks::TORCHFLOWER_CROP);
        assert_eq!(age_one.get_value(&BlockStateProperties::AGE_1), 1);

        let mature = behavior.get_state_for_age(2);
        assert_eq!(mature.get_block(), &vanilla_blocks::TORCHFLOWER);
    }
}
