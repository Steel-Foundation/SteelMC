use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::{BlockRef, default_surviving_state};

const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Vanilla `ChorusFlowerBlock` survival behavior.
// TODO: Implement ticking, projectile break behavior, and growth outside worldgen.
#[block_behavior]
pub struct ChorusFlowerBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    plant: BlockRef,
}

impl ChorusFlowerBlock {
    /// Creates a new chorus flower block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, plant: BlockRef) -> Self {
        Self { block, plant }
    }
}

impl BlockBehavior for ChorusFlowerBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_state = world.get_block_state(pos.below());
        if below_state.get_block() == self.plant
            || below_state
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_CHORUS_FLOWER)
        {
            return true;
        }

        if !below_state.is_air() {
            return false;
        }

        let mut has_single_plant_neighbor = false;
        for direction in HORIZONTAL_DIRECTIONS {
            let neighbor_state = world.get_block_state(pos.relative(direction));
            if neighbor_state.get_block() == self.plant {
                if has_single_plant_neighbor {
                    return false;
                }
                has_single_plant_neighbor = true;
            } else if !neighbor_state.is_air() {
                return false;
            }
        }

        has_single_plant_neighbor
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
}
