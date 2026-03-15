use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        properties::{BlockStateProperties, IntProperty},
    },
    item_stack::ItemStack,
    vanilla_items,
};
use steel_utils::{BlockPos, BlockStateId};

use crate::{
    behavior::blocks::vegetation::{
        bonemealable::{Bonemealable, CropBonemealExt},
        crop_block::CropLike,
    },
    world::World,
};

/// Behavior for Carrots
#[block_behavior]
pub struct CarrotBlock {
    block: BlockRef,
}

impl CarrotBlock {
    /// Creates a new Potato Block Behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl CropLike for CarrotBlock {
    fn block(&self) -> BlockRef {
        self.block
    }

    fn age_property(&self) -> &IntProperty {
        &BlockStateProperties::AGE_7
    }

    fn max_age(&self) -> u8 {
        7
    }

    fn clone_item_stack(&self) -> ItemStack {
        ItemStack::new(&vanilla_items::ITEMS.carrot)
    }
}

impl Bonemealable for CarrotBlock {
    fn get_age_increase(&self, _world: &Arc<World>) -> u8 {
        rand::random_range(2..=5)
    }
    fn is_bonemealable(&self, state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) -> bool {
        !self.is_max_age(state)
    }

    fn apply_bonemeal(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.default_apply_bonemeal(state, world, pos);
    }
}
