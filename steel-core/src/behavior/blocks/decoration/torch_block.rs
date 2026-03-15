//! Torch block implementations.
//!
//! Torches come in two forms:
//! - Standing torches (TorchBlock): placed on top of blocks
//! - Wall torches (WallTorchBlock): placed on the side of blocks
//!
//! Both break when their supporting block is removed.

use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Behavior for standing torch blocks (torch, `soul_torch`, `copper_torch`).
///
/// Standing torches are placed on top of blocks and require center support
/// from the block below to survive.
#[block_behavior]
pub struct TorchBlock {
    block: BlockRef,
}

impl TorchBlock {
    /// Creates a new standing torch block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for TorchBlock {
    /// Checks if a torch can survive at the given position.
    /// Requires the block below to provide center support on its top face.
    fn can_survive(&self, _state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = world.get_block_state(&below_pos);
        below_state.is_face_sturdy_for(Direction::Up, SupportType::Center)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Standing torches break when the block below is removed
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_state = self.block.default_state();
        // Check if we can place on the block below
        if !self.can_survive(default_state, context.world, context.relative_pos) {
            return None;
        }

        Some(default_state)
    }
}

/// Behavior for wall torch blocks (`wall_torch`, `soul_wall_torch`, `copper_wall_torch`).
///
/// Wall torches are placed on the side of blocks and require a sturdy face
/// from the block they're attached to.
#[block_behavior]
pub struct WallTorchBlock {
    block: BlockRef,
}

impl WallTorchBlock {
    /// Creates a new wall torch block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehaviour for WallTorchBlock {
    /// Checks if a wall torch can survive at the given position.
    /// Requires the block behind (opposite of facing) to provide a sturdy face.
    fn can_survive(&self, state: BlockStateId, world: &World, pos: BlockPos) -> bool {
        let facing: Direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let attach_direction = facing.opposite();
        let attach_pos = attach_direction.relative(&pos);
        let attach_state = world.get_block_state(&attach_pos);
        attach_state.is_face_sturdy(facing)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Wall torches break when the block they're attached to is removed
        let facing: Direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let attach_direction = facing.opposite();

        if direction == attach_direction && !self.can_survive(state, world, pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Vanilla iterates through getNearestLookingDirections() and uses the opposite
        // of each horizontal direction as the facing (torch points away from wall)
        for direction in context.get_nearest_looking_directions() {
            if direction.is_horizontal() {
                let facing = direction.opposite();
                let state = self
                    .block
                    .default_state()
                    .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing);
                if self.can_survive(state, context.world, context.relative_pos) {
                    return Some(state);
                }
            }
        }

        None
    }
}
