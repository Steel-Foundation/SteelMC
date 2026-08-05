use std::sync::Arc;

use rand::RngExt;
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
    world::{LevelReader, World},
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
        ItemStack::new(&vanilla_items::CARROT)
    }
}

impl Bonemealable for CarrotBlock {
    fn get_bonemeal_age_increase(&self, _world: &Arc<World>, rng: &mut dyn rand::Rng) -> u8 {
        rng.random_range(2..=5)
    }
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        !self.is_max_age(state)
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        self.default_perform_bonemeal(state, world, rng, pos);
    }
}
