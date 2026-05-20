//! Cactus flower block behavior.
//!
//! Cactus flower is a vegetation block that can be placed on cactus, farmland,
//! or any block with a sturdy center face on top.
<<<<<<< HEAD
<<<<<<< HEAD:steel-core/src/behavior/blocks/vegetation/cactus_flower_block.rs
=======
//!
//! Vanilla equivalent: `CactusFlowerBlock` extends `VegetationBlock`.
>>>>>>> 3643c5b7e (Add worldgen features stage (#183)):steel-core/src/behavior/blocks/farming/cactus_flower_block.rs
=======
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_registry::{TaggedRegistryExt, vanilla_block_tags};
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::{LevelReader, ScheduledTickAccess};

/// Behavior for cactus flower blocks.
#[block_behavior]
pub struct CactusFlowerBlock {
    block: BlockRef,
}

impl CactusFlowerBlock {
    /// Creates a new cactus flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for CactusFlowerBlock {
    /// Checks if the block below can support a cactus flower.
    ///
    /// Vanilla `CactusFlowerBlock.mayPlaceOn`: accepts the support-override tag
    /// or any block with a sturdy center face on top.
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
<<<<<<< HEAD
<<<<<<< HEAD:steel-core/src/behavior/blocks/vegetation/cactus_flower_block.rs
        let below = world.get_block_state(pos.below());
=======
        let below_pos = pos.below();
        let below = world.get_block_state(below_pos);
        let below_block = below.get_block();

>>>>>>> 3643c5b7e (Add worldgen features stage (#183)):steel-core/src/behavior/blocks/farming/cactus_flower_block.rs
=======
        let below = world.get_block_state(pos.below());
>>>>>>> a3a9bf85f (Crops and Bonemeal (#116))
        steel_registry::REGISTRY.blocks.is_in_tag(
            below.get_block(),
            &vanilla_block_tags::SUPPORT_OVERRIDE_CACTUS_FLOWER_TAG,
        ) || below.is_face_sturdy_for(Direction::Up, SupportType::Center)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world, context.relative_pos)
            .then_some(state)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if self.can_survive(state, world, pos) {
            state
        } else {
            vanilla_blocks::AIR.default_state()
        }
    }
}
