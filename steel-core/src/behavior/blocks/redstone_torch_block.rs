//! Redstone torch behaviors (standing and wall variants).
//!
//! These mirror the placement/survival rules of regular torches but add a `LIT`
//! property and are intended to be expanded with redstone logic later.

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::blocks::{TorchBlock, WallTorchBlock};
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Standing redstone torch (`redstone_torch`).
///
/// TODO: Redstone functionality (signal output, neighbor notifications,
/// scheduled ticks, burnout, particle effects).
pub struct RedstoneTorchBlock {
    block: BlockRef,
}

impl RedstoneTorchBlock {
    #[must_use]
    /// Creates a new standing redstone torch behavior for the given block ref.
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn can_survive(world: &World, pos: BlockPos) -> bool {
        TorchBlock::can_survive(world, pos)
    }
}

impl BlockBehaviour for RedstoneTorchBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down && !Self::can_survive(world, pos) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        if !Self::can_survive(context.world, context.relative_pos) {
            return None;
        }
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::LIT, true),
        )
    }

    // TODO: implement redstone signal source behavior, neighbor updates, and burnout.
}

/// Wall redstone torch (`redstone_wall_torch`).
///
/// TODO: Redstone functionality (signal output by facing, neighbor notifications,
/// scheduled ticks, burnout, particle effects).
pub struct RedstoneWallTorchBlock {
    block: BlockRef,
}

impl RedstoneWallTorchBlock {
    #[must_use]
    /// Creates a new wall redstone torch behavior for the given block ref.
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn can_survive(world: &World, pos: BlockPos, facing: Direction) -> bool {
        WallTorchBlock::can_survive(world, pos, facing)
    }
}

impl BlockBehaviour for RedstoneWallTorchBlock {
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let facing: Direction = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let attach_direction = facing.opposite();

        if direction == attach_direction && !Self::can_survive(world, pos, facing) {
            return REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        }
        state
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let clicked_face = context.clicked_face;
        if clicked_face.is_horizontal() {
            let facing = clicked_face;
            if Self::can_survive(context.world, context.relative_pos, facing) {
                return Some(
                    self.block
                        .default_state()
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
                        .set_value(&BlockStateProperties::LIT, true),
                );
            }
        }

        for &facing in &[
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ] {
            if Self::can_survive(context.world, context.relative_pos, facing) {
                return Some(
                    self.block
                        .default_state()
                        .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
                        .set_value(&BlockStateProperties::LIT, true),
                );
            }
        }

        None
    }

    // TODO: implement redstone signal source behavior, neighbor updates, and burnout.
}
