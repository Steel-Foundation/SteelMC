use std::sync::Arc;

use steel_registry::{
    REGISTRY,
    blocks::{BlockRef, properties::IntProperty},
    item_stack::ItemStack,
};
use steel_utils::{BlockStateId, Identifier};

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext,
        blocks::vegetation::{
            Vegetation,
            bonemealable::{Bonemealable, CropBonemealExt},
            crop_block::CropLike,
            vegetation_block::{vegetation_can_survive, vegetation_update_shape},
        },
    },
    world::World,
};

/// Behavior for the Torchflower Block
pub struct TorchflowerBlock {
    block: BlockRef,
    age_property: IntProperty,
    max_age: u8,
    clone_item: Identifier,
}

impl TorchflowerBlock {
    /// Creates a new crop block behavior with a custom age property.
    #[must_use]
    pub const fn with_age(
        block: BlockRef,
        age_property: IntProperty,
        max_age: u8,
        clone_item: &'static str,
    ) -> Self {
        Self {
            block,
            age_property,
            max_age,
            clone_item: Identifier::vanilla_static(clone_item),
        }
    }
}

impl CropLike for TorchflowerBlock {
    fn block(&self) -> BlockRef {
        self.block
    }

    fn age_property(&self) -> &IntProperty {
        &self.age_property
    }

    fn max_age(&self) -> u8 {
        self.max_age
    }
}

impl Bonemealable for TorchflowerBlock {
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

impl BlockBehaviour for TorchflowerBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if self.may_place_on(
            context.world.get_block_state(&context.relative_pos.below()),
            context.world,
            context.relative_pos.below(),
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    fn can_survive(&self, state: BlockStateId, world: &World, pos: steel_utils::BlockPos) -> bool {
        vegetation_can_survive(self, state, world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: steel_utils::BlockPos,
        _direction: steel_utils::Direction,
        _neighbor_pos: steel_utils::BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        vegetation_update_shape(self, state, world, pos)
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        !self.is_max_age(state)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: steel_utils::BlockPos) {
        if rand::random_range(0..3) != 0 {
            self.on_random_tick(state, world, pos);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        REGISTRY.items.by_key(&self.clone_item).map(ItemStack::new)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
