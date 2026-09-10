use super::snowy_block::is_snowy_setting;
use crate::behavior::BlockRef;
use crate::world::{LevelReader, World};
use std::sync::Arc;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_registry::{blocks::block_state_ext::BlockStateExt, vanilla_fluid_tags::FluidTag};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

pub(super) struct SpreadingSnowyBlock {
    own_block: BlockRef,
    base_block: BlockRef,
}

impl SpreadingSnowyBlock {
    pub(super) const fn new(own: BlockRef, base: BlockRef) -> Self {
        SpreadingSnowyBlock {
            own_block: own,
            base_block: base,
        }
    }

    fn can_stay_alive(_state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
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
        light_dampening_top_face < 15
    }
    fn can_propagate(state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
        Self::can_stay_alive(state, level, pos)
            && !level
                .get_block_state(pos.above())
                .get_fluid_state()
                .fluid_id
                .has_tag(&FluidTag::WATER)
    }
    pub(super) fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !Self::can_stay_alive(state, world, pos) {
            world.set_block(
                pos,
                self.base_block.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        } else if world.max_local_raw_brightness(pos.above(), world.sky_darkening()) >= 9 {
            let default_block_state = self.own_block.default_state();

            for _ in 0..4 {
                let test_pos = pos.offset(
                    rand::random_range(-1..2),
                    rand::random_range(-3..2),
                    rand::random_range(-1..2),
                );
                if world.get_block_state(test_pos).get_block() == self.base_block
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
