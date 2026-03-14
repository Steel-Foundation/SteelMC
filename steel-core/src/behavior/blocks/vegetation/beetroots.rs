use std::sync::Arc;

use steel_registry::blocks::{BlockRef, properties::IntProperty};
use steel_utils::BlockStateId;

use crate::{
    behavior::{
        BlockBehaviour, BlockPlaceContext,
        blocks::vegetation::{
            bonemealable::{Bonemealable, CropBonemealExt},
            crop_block::CropLike,
        },
    },
    world::World,
};

/// Behavior for the Beetroots Block
pub struct BeetrootBlock {
    block: BlockRef,
    age_property: IntProperty,
    max_age: u8,
}

impl BeetrootBlock {
    /// Creates a new crop block behavior with a custom age property.
    #[must_use]
    pub const fn with_age(block: BlockRef, age_property: IntProperty, max_age: u8) -> Self {
        Self {
            block,
            age_property,
            max_age,
        }
    }
}

impl CropLike for BeetrootBlock {
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

impl Bonemealable for BeetrootBlock {
    fn get_age_increase(&self, _world: &World) -> u8 {
        rand::random_range(2..=5) / 3
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

impl BlockBehaviour for BeetrootBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.get_state_for_age(0))
    }

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        !self.is_max_age(state)
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: steel_utils::BlockPos) {
        if rand::random_range(0..3) != 0 {
            self.on_random_tick(state, world, pos);
        }
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
