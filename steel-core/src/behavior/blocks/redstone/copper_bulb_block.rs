//! Vanilla copper-bulb edge-triggered behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::sound_events;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::blocks::{WeatherState, WeatheringCopper};
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, SignalGetter as _, World};

/// Vanilla `CopperBulbBlock`, used directly by waxed bulb variants.
#[block_behavior]
pub struct CopperBulbBlock {
    block: BlockRef,
}

impl CopperBulbBlock {
    /// Creates copper-bulb behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn check_and_flip(state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let signal = world.has_neighbor_signal(pos);
        let powered = state.get_value(&BlockStateProperties::POWERED);
        if signal == powered {
            return;
        }

        let mut new_state = state;
        if !powered {
            let lit = !state.get_value(&BlockStateProperties::LIT);
            new_state = new_state.set_value(&BlockStateProperties::LIT, lit);
            world.play_block_sound(
                if lit {
                    &sound_events::BLOCK_COPPER_BULB_TURN_ON
                } else {
                    &sound_events::BLOCK_COPPER_BULB_TURN_OFF
                },
                pos,
                1.0,
                1.0,
                None,
            );
        }
        world.set_block(
            pos,
            new_state.set_value(&BlockStateProperties::POWERED, signal),
            UpdateFlags::UPDATE_ALL,
        );
    }

    fn placed(state: BlockStateId, world: &Arc<World>, pos: BlockPos, old_state: BlockStateId) {
        if old_state.get_block() != state.get_block() {
            Self::check_and_flip(state, world, pos);
        }
    }

    fn analog_output(world: &dyn LevelReader, pos: BlockPos) -> i32 {
        if world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::LIT)
        {
            15
        } else {
            0
        }
    }
}

impl BlockBehavior for CopperBulbBlock {
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
        Self::placed(state, world, pos, old_state);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        Self::check_and_flip(state, world, pos);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        Self::analog_output(world, pos)
    }
}

/// Unwaxed copper bulbs with vanilla oxidation behavior.
#[block_behavior]
pub struct WeatheringCopperBulbBlock {
    bulb: CopperBulbBlock,
    #[json_arg(r#enum = "WeatherState", json = "weather_state")]
    weathering: WeatheringCopper,
}

impl WeatheringCopperBulbBlock {
    /// Creates a weathering copper-bulb behavior.
    #[must_use]
    pub const fn new(block: BlockRef, weather_state: WeatherState) -> Self {
        Self {
            bulb: CopperBulbBlock::new(block),
            weathering: WeatheringCopper::new(weather_state),
        }
    }
}

impl BlockBehavior for WeatheringCopperBulbBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.bulb.get_state_for_placement(context)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        CopperBulbBlock::placed(state, world, pos, old_state);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        CopperBulbBlock::check_and_flip(state, world, pos);
    }

    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.weathering.change_over_time(state, world, pos);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        CopperBulbBlock::analog_output(world, pos)
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn bulb_toggles_lit_only_on_rising_edges() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("copper_bulb_edges");
        let pos = BlockPos::new(8, 64, 8);
        let power_pos = pos.west();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            pos,
            vanilla_blocks::WAXED_COPPER_BULB.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let first_rise = world.get_block_state(pos);
        assert!(first_rise.get_value(&BlockStateProperties::POWERED));
        assert!(first_rise.get_value(&BlockStateProperties::LIT));
        let behavior = BLOCK_BEHAVIORS.get_behavior(first_rise.get_block());
        assert_eq!(
            behavior.get_analog_output_signal(first_rise, &world, pos, Direction::North),
            15
        );

        assert!(world.remove_block(power_pos, false));
        let falling = world.get_block_state(pos);
        assert!(!falling.get_value(&BlockStateProperties::POWERED));
        assert!(falling.get_value(&BlockStateProperties::LIT));

        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        let second_rise = world.get_block_state(pos);
        assert!(second_rise.get_value(&BlockStateProperties::POWERED));
        assert!(!second_rise.get_value(&BlockStateProperties::LIT));
    }
}
