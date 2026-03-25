//! Sugar Cane block behavior.
//!
//! Sugar cane grows up to 3 blocks tall via random ticks. It requires water adjacent
//! to the block it is planted on (or frosted ice).

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::FluidStateExt;
use steel_registry::vanilla_blocks;
use steel_registry::{REGISTRY, TaggedRegistryExt};
use steel_utils::{BlockPos, BlockStateId, Identifier, types::UpdateFlags};

use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{BlockBehavior, BlockStateBehaviorExt};
use crate::world::World;

/// Maximum sugar cane stack height (vanilla: 3 blocks).
const MAX_SUGAR_CANE_HEIGHT: i32 = 3;

/// Behavior for sugar cane blocks.
#[block_behavior]
pub struct SugarCaneBlock {
    block: BlockRef,
}

impl SugarCaneBlock {
    /// Creates a new sugar cane block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SugarCaneBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.relative_pos;
        if self.can_survive(
            vanilla_blocks::SUGAR_CANE.default_state(), // state argument is unused
            context.world,
            pos,
        ) {
            Some(self.block.default_state())
        } else {
            None
        }
    }

    /// Called when this block is placed.
    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if state.get_block() == old_state.get_block() {
            return;
        }

        if !self.can_survive(state, world, pos) {
            world.schedule_block_tick_default(pos, state.get_block(), 1);
        }
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let above_pos = pos.above();

        if !world.get_block_state(above_pos).is_air() {
            return;
        }

        let mut height = 1i32;
        while world.get_block_state(pos.below_n(height)).get_block() == self.block {
            height += 1;
        }

        if height >= MAX_SUGAR_CANE_HEIGHT {
            return;
        }

        let age = state.get_value(&BlockStateProperties::AGE_15);

        if age == 15 {
            world.set_block(
                above_pos,
                self.block.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
            let new_state = state.set_value(&BlockStateProperties::AGE_15, 0);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        } else {
            let new_state = state.set_value(&BlockStateProperties::AGE_15, age + 1);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if !self.can_survive(state, world, pos) {
            return vanilla_blocks::AIR.default_state();
        }
        state
    }

    fn can_survive(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        let below_block = below_state.get_block();

        if below_block == vanilla_blocks::SUGAR_CANE {
            return true;
        }

        let is_valid_ground = REGISTRY
            .blocks
            .is_in_tag(below_block, &Identifier::vanilla_static("dirt"))
            || REGISTRY
                .blocks
                .is_in_tag(below_block, &Identifier::vanilla_static("sand"));

        if !is_valid_ground {
            return false;
        }

        for dir in [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let neighbor_pos = dir.relative(below_pos);
            let neighbor_state = world.get_block_state(neighbor_pos);
            let neighbor_block = neighbor_state.get_block();

            if neighbor_state.get_fluid_state().is_water()
                || neighbor_block == vanilla_blocks::FROSTED_ICE
            {
                return true;
            }
        }

        false
    }
}
