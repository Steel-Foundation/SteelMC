//! Sugar Cane block behavior.
//!
//! Sugar cane grows up to 3 blocks tall via random ticks. It requires water adjacent
//! to the block it is planted on (or frosted ice).

use std::ptr;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Identifier, types::UpdateFlags};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Maximum sugar cane stack height (vanilla: 3 blocks).
const MAX_SUGAR_CANE_HEIGHT: i32 = 3;

/// Behavior for sugar cane blocks.
pub struct SugarCaneBlock {
    block: BlockRef,
}

impl SugarCaneBlock {
    /// Creates a new sugar cane block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if sugar cane can survive at the given position.
    ///
    /// Survival requirements:
    /// 1. Block below is Sugar Cane (stacking), OR
    /// 2. Block below is Dirt/Grass/Sand AND adjacent to Water/FrostedIce.
    fn can_survive(world: &World, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = world.get_block_state(&below_pos);
        let below_block = below_state.get_block();

        // 1. Check if placing on top of another sugar cane
        if ptr::eq(below_block, vanilla_blocks::SUGAR_CANE) {
            return true;
        }

        // 2. Check if placing on valid ground (Dirt or Sand tags)
        let is_valid_ground = REGISTRY
            .blocks
            .is_in_tag(below_block, &Identifier::vanilla_static("dirt"))
            || REGISTRY
                .blocks
                .is_in_tag(below_block, &Identifier::vanilla_static("sand"));

        if is_valid_ground {
            // Check for adjacent water or frosted ice around the *ground* block
            for dir in [
                Direction::North,
                Direction::South,
                Direction::East,
                Direction::West,
            ] {
                let neighbor_pos = dir.relative(&below_pos);
                let neighbor_state = world.get_block_state(&neighbor_pos);
                let neighbor_block = neighbor_state.get_block();

                // Check for water fluid or frosted ice block
                // TODO: Proper fluid check using fluid tags
                if !neighbor_state.get_fluid_state().is_empty() {
                    return true;
                }

                if ptr::eq(neighbor_block, vanilla_blocks::FROSTED_ICE) {
                    return true;
                }
            }
        }

        false
    }
}

impl BlockBehaviour for SugarCaneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.relative_pos;
        if Self::can_survive(context.world, pos) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    /// Called when this block is placed.
    fn on_place(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if ptr::eq(state.get_block(), old_state.get_block()) {
            return;
        }

        // HACK: Immediate destruction instead of vanilla's scheduled tick
        if !Self::can_survive(world, pos) {
            world.destroy_block_effect(pos, u32::from(state.0), None);
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let above_pos = pos.above();

        if !world.get_block_state(&above_pos).is_air() {
            return;
        }

        // Count sugar cane blocks below to determine height
        let mut height = 1i32;
        while ptr::eq(
            world.get_block_state(&pos.below_n(height)).get_block(),
            self.block,
        ) {
            height += 1;
        }

        if height < MAX_SUGAR_CANE_HEIGHT {
            let age = state.get_value(&BlockStateProperties::AGE_15);

            if age == 15 {
                world.set_block(
                    above_pos,
                    self.block.default_state(),
                    UpdateFlags::UPDATE_ALL,
                );
                // Reset age
                let new_state = state.set_value(&BlockStateProperties::AGE_15, 0);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            } else {
                let new_state = state.set_value(&BlockStateProperties::AGE_15, age + 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            }
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !Self::can_survive(world, pos) {
            world.destroy_block_effect(pos, u32::from(state.0), None);

            // TODO: Drop sugar cane item via pop_resource
            return vanilla_blocks::AIR.default_state();
        }
        state
    }
}
