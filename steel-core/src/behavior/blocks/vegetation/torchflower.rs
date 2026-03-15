use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        properties::{BlockStateProperties, IntProperty},
    },
    item_stack::ItemStack,
    vanilla_items,
};
use steel_utils::BlockStateId;

use crate::{
    behavior::blocks::vegetation::{
        bonemealable::{Bonemealable, CropBonemealExt},
        crop_block::CropLike,
    },
    world::World,
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
        &BlockStateProperties::AGE_2
    }

    fn max_age(&self) -> u8 {
        2
    }

    fn clone_item_stack(&self) -> ItemStack {
        ItemStack::new(&vanilla_items::ITEMS.torchflower_seeds)
    }

    fn should_random_tick(&self) -> bool {
        rand::random_range(0..3) != 0
    }
}

impl Bonemealable for TorchflowerCropBlock {
    fn get_age_increase(&self, _world: &World) -> u8 {
        1
    }

    fn is_bonemealable(
        &self,
        state: BlockStateId,
        _world: &World,
        _pos: steel_utils::BlockPos,
    ) -> bool {
        !self.is_max_age(state)
    }

    fn apply_bonemeal(&self, state: BlockStateId, world: &World, pos: steel_utils::BlockPos) {
        self.default_apply_bonemeal(state, world, pos);
    }
}
