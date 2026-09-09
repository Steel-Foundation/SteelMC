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

    fn can_stay_alive(&self, _state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
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

        let light_dampening_top_face = above_state.get_light_dampening();
        light_dampening_top_face < 15
    }
    fn can_propagate(&self, state: BlockStateId, level: &Arc<World>, pos: BlockPos) -> bool {
        let above = pos.above();
        return self.can_stay_alive(state, level, pos)
            && !level
                .get_block_state(above)
                .get_fluid_state()
                .fluid_id
                .has_tag(&FluidTag::WATER);
    }
    pub(super) fn random_tick(&self, state: BlockStateId, level: &Arc<World>, pos: BlockPos) {
        if !self.can_stay_alive(state, level, pos) {
            level.set_block(
                pos,
                self.base_block.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        } else if level.max_local_raw_brightness(pos.above(), level.sky_darkening()) >= 9 {
            let default_block_state = self.own_block.default_state();

            for _ in 0..4 {
                // This copies the java code, but maybe we should change this to have a range of -1..2
                let test_pos = pos.offset(
                    rand::random_range(0..3) - 1,
                    rand::random_range(0..5) - 3,
                    rand::random_range(0..3) - 1,
                );
                if level.get_block_state(test_pos).get_block() == self.base_block
                    && self.can_propagate(default_block_state, level, test_pos)
                {
                    level.set_block(
                        test_pos,
                        default_block_state.set_value(
                            &BlockStateProperties::SNOWY,
                            is_snowy_setting(level.get_block_state(test_pos.above())),
                        ),
                        UpdateFlags::UPDATE_ALL,
                    );
                }
            }
        }
    }
}
