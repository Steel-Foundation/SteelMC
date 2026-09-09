//! `NetherrackBlock` behavior (`net.minecraft.world.level.block.NetherrackBlock`).

use std::sync::Arc;

use rand::RngExt;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, World};

/// Chance to pick `Warped Nylium` when both nylium types are nearby.
const WARPED_VS_CRIMSON_CHANCE: f64 = 0.5;

/// Vanilla `NetherrackBlock`.
#[block_behavior]
pub struct NetherrackBlock {
    block: BlockRef,
}

impl NetherrackBlock {
    /// Creates the behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for NetherrackBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for NetherrackBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        if world.get_block_state(pos.above()).get_light_dampening() != 0 {
            return false;
        }
        for check_pos in BlockPos::between_closed(pos.offset(-1, -1, -1), pos.offset(1, 1, 1)) {
            if world
                .get_block_state(check_pos)
                .get_block()
                .has_tag(&BlockTag::NYLIUM)
            {
                return true;
            }
        }
        false
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        rng: &mut dyn rand::Rng,
        pos: BlockPos,
    ) {
        let mut found_warped = false;
        let mut found_crimson = false;
        for check_pos in BlockPos::between_closed(pos.offset(-1, -1, -1), pos.offset(1, 1, 1)) {
            let state = world.get_block_state(check_pos);
            let block = state.get_block();
            if block == &vanilla_blocks::WARPED_NYLIUM {
                found_warped = true;
            }
            if block == &vanilla_blocks::CRIMSON_NYLIUM {
                found_crimson = true;
            }
            if found_warped && found_crimson {
                break;
            }
        }
        let new_state = if found_warped && found_crimson {
            if rng.random_bool(WARPED_VS_CRIMSON_CHANCE) {
                vanilla_blocks::WARPED_NYLIUM.default_state()
            } else {
                vanilla_blocks::CRIMSON_NYLIUM.default_state()
            }
        } else if found_warped {
            vanilla_blocks::WARPED_NYLIUM.default_state()
        } else if found_crimson {
            vanilla_blocks::CRIMSON_NYLIUM.default_state()
        } else {
            return;
        };
        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::NeighborSpreader
    }
}
