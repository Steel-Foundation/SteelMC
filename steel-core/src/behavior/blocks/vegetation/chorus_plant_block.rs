use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::BlockPlaceContext;
use crate::world::LevelReader;

use super::BlockRef;

const HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Vanilla `ChorusPlantBlock` connection and survival behavior.
// TODO: Implement ticking and full shape-update side effects.
#[block_behavior]
pub struct ChorusPlantBlock {
    block: BlockRef,
}

impl ChorusPlantBlock {
    /// Creates a new chorus plant block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    #[must_use]
    pub(crate) fn state_with_connections(
        world: &dyn LevelReader,
        pos: BlockPos,
        mut state: BlockStateId,
    ) -> BlockStateId {
        let down = world.get_block_state(pos.below());
        let up = world.get_block_state(pos.above());
        let north = world.get_block_state(pos.north());
        let east = world.get_block_state(pos.east());
        let south = world.get_block_state(pos.south());
        let west = world.get_block_state(pos.west());
        let block = state.get_block();

        state = state.set_value(
            &BlockStateProperties::DOWN,
            down.get_block() == block
                || down.get_block() == &vanilla_blocks::CHORUS_FLOWER
                || down.get_block().has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT),
        );
        state = state.set_value(
            &BlockStateProperties::UP,
            up.get_block() == block || up.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            &BlockStateProperties::NORTH,
            north.get_block() == block || north.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            &BlockStateProperties::EAST,
            east.get_block() == block || east.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state = state.set_value(
            &BlockStateProperties::SOUTH,
            south.get_block() == block || south.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        );
        state.set_value(
            &BlockStateProperties::WEST,
            west.get_block() == block || west.get_block() == &vanilla_blocks::CHORUS_FLOWER,
        )
    }
}

impl BlockBehavior for ChorusPlantBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_state = world.get_block_state(pos.below());
        let block_above_or_below =
            !world.get_block_state(pos.above()).is_air() && !below_state.is_air();

        for direction in HORIZONTAL_DIRECTIONS {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = world.get_block_state(neighbor_pos);
            if neighbor_state.get_block() == self.block {
                if block_above_or_below {
                    return false;
                }

                let below = world.get_block_state(neighbor_pos.below());
                if below.get_block() == self.block
                    || below.get_block().has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT)
                {
                    return true;
                }
            }
        }

        below_state.get_block() == self.block
            || below_state
                .get_block()
                .has_tag(&BlockTag::SUPPORTS_CHORUS_PLANT)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(Self::state_with_connections(
            context.world.as_ref(),
            context.relative_pos,
            self.block.default_state(),
        ))
    }
}
