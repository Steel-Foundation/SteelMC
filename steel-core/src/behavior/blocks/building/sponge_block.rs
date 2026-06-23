use std::sync::Arc;

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::{sound_events, vanilla_blocks, vanilla_fluid_tags};
use steel_utils::{
    BlockPos, BlockStateId,
    types::{TraversalNodeStatus, UpdateFlags},
};

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt,
    pickup_waterlogged_block,
};
use crate::world::World;

const MAX_DEPTH: i32 = 6;
const MAX_COUNT: i32 = 64;

#[block_behavior]
/// Sponge behavior
pub struct SpongeBlock {
    block: BlockRef,
}

impl SpongeBlock {
    /// Creates a new sponge block
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn try_absorb_water(world: &Arc<World>, pos: BlockPos) {
        if !Self::remove_water_breadth_first_search(world, pos) {
            return;
        }

        world.set_block(
            pos,
            vanilla_blocks::WET_SPONGE.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        );
        world.play_sound(
            &sound_events::BLOCK_SPONGE_ABSORB,
            SoundSource::Blocks,
            pos,
            1.0,
            1.0,
            None,
        );
    }

    fn remove_water_breadth_first_search(world: &Arc<World>, start_pos: BlockPos) -> bool {
        BlockPos::breadth_first_traversal(
            start_pos,
            MAX_DEPTH,
            MAX_COUNT + 1,
            |pos, consumer| {
                for direction in Direction::ALL {
                    consumer(pos.relative(direction));
                }
            },
            |pos| {
                if pos == start_pos || Self::remove_water_at(world, pos) {
                    TraversalNodeStatus::Accept
                } else {
                    TraversalNodeStatus::Skip
                }
            },
        ) > 1
    }

    fn remove_water_at(world: &Arc<World>, pos: BlockPos) -> bool {
        let state = world.get_block_state(pos);
        let fluid_state = state.get_fluid_state();
        if !fluid_state
            .fluid_id
            .has_tag(&vanilla_fluid_tags::FluidTag::WATER)
        {
            return false;
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        if behavior.pickup_block(world, pos, state, None).is_some() {
            return true;
        }

        if pickup_waterlogged_block(behavior, world, pos, state, None).is_some() {
            return true;
        }

        if state.get_block() == &vanilla_blocks::WATER {
            return world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }

        if !Self::is_absorbable_water_plant(state) {
            return false;
        }

        world.drop_resources(state, pos);
        world.set_block(
            pos,
            vanilla_blocks::AIR.default_state(),
            UpdateFlags::UPDATE_ALL,
        )
    }

    fn is_absorbable_water_plant(state: BlockStateId) -> bool {
        let block = state.get_block();
        block == &vanilla_blocks::KELP
            || block == &vanilla_blocks::KELP_PLANT
            || block == &vanilla_blocks::SEAGRASS
            || block == &vanilla_blocks::TALL_SEAGRASS
    }
}

impl BlockBehavior for SpongeBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != state.get_block() {
            Self::try_absorb_water(world, pos);
        }
    }

    fn handle_neighbor_changed(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        Self::try_absorb_water(world, pos);
    }
}
