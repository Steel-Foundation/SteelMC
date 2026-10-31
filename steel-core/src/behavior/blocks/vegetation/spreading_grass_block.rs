use super::snowy_block::is_snowy_setting;
use crate::behavior::BlockRef;
use crate::chunk::light::{MAX_LIGHT_LEVEL, get_light_block_into};
use crate::world::{LevelReader, World};
use std::sync::Arc;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::entity_data::Direction;
use steel_registry::vanilla_blocks;
use steel_registry::{blocks::block_state_ext::BlockStateExt, vanilla_fluid_tags::FluidTag};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

/// A structure implementing the spreading of grass blocks and its variants
pub struct SpreadingGrassBlock {}

impl SpreadingGrassBlock {
    fn can_stay_alive(state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
        let above = pos.above();
        let above_state: BlockStateId = level.get_block_state(above);
        if above_state.get_block() == &vanilla_blocks::SNOW
            && above_state.get_value(&BlockStateProperties::LAYERS) == 1
        {
            return true;
        }
        if above_state.get_fluid_state().is_full() {
            return false;
        }

        let light_dampening_top_face = get_light_block_into(
            state,
            above_state,
            Direction::Up,
            above_state.get_light_dampening(),
        );
        light_dampening_top_face < MAX_LIGHT_LEVEL
    }
    fn can_propagate(state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
        Self::can_stay_alive(state, level, pos)
            && !level
                .get_block_state(pos.above())
                .get_fluid_state()
                .fluid_id
                .has_tag(&FluidTag::WATER)
    }

    /// Implements random tick for grass block and its variants (like mycelium)
    /// It allows blocks to spread to base blocks, and will make them disappear if there is any block ontop of them
    pub fn random_tick(
        own: BlockRef,
        base: BlockRef,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) {
        if !Self::can_stay_alive(state, world, pos) {
            world.set_block(pos, base.default_state(), UpdateFlags::UPDATE_ALL);
        } else if world.max_local_raw_brightness(pos.above(), world.sky_darkening()) >= 9 {
            let default_block_state = own.default_state();

            for _ in 0..4 {
                let test_pos = pos.offset(
                    rand::random_range(-1..2),
                    rand::random_range(-3..2),
                    rand::random_range(-1..2),
                );
                if world.get_block_state(test_pos).get_block() == base
                    && Self::can_propagate(default_block_state, world, test_pos)
                {
                    world.set_block(
                        test_pos,
                        default_block_state.set_value(
                            &BlockStateProperties::SNOWY,
                            is_snowy_setting(world.get_block_state(test_pos.above())),
                        ),
                        UpdateFlags::UPDATE_ALL,
                    );
                }
            }
        }
    }
}
