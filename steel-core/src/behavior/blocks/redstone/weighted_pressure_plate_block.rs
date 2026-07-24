//! Vanilla weighted pressure-plate behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::sound_event::SoundEventRef;
use steel_utils::{BlockPos, BlockStateId};

use super::base_pressure_plate_block::BasePressurePlateBlock;
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

const PRESSED_TIME: i32 = 10;

/// Vanilla weighted pressure plate with analog output based on entity count.
#[block_behavior]
pub struct WeightedPressurePlateBlock {
    base: BasePressurePlateBlock,
    #[json_arg(value)]
    max_weight: i32,
    #[json_arg(sound_events, json = "type_pressure_plate_click_on")]
    sound_click_on: SoundEventRef,
    #[json_arg(sound_events, json = "type_pressure_plate_click_off")]
    sound_click_off: SoundEventRef,
}

impl WeightedPressurePlateBlock {
    /// Creates a weighted pressure plate from extracted vanilla data.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        max_weight: i32,
        sound_click_on: SoundEventRef,
        sound_click_off: SoundEventRef,
    ) -> Self {
        Self {
            base: BasePressurePlateBlock::new(block),
            max_weight,
            sound_click_on,
            sound_click_off,
        }
    }

    fn signal_for_state(state: BlockStateId) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::POWER))
    }

    fn state_for_signal(state: BlockStateId, signal: i32) -> BlockStateId {
        state.set_value(&BlockStateProperties::POWER, signal as u8)
    }

    fn signal_for_count(count: i32, max_weight: i32) -> i32 {
        let count = count.min(max_weight);
        if count <= 0 {
            return 0;
        }
        (((count as f32) / (max_weight as f32)) * 15.0).ceil() as i32
    }

    fn signal_strength(&self, world: &World, pos: BlockPos) -> i32 {
        let count = BasePressurePlateBlock::entity_count(world, pos, |_| true);
        let count = i32::try_from(count).unwrap_or(i32::MAX);
        Self::signal_for_count(count, self.max_weight)
    }

    fn check_pressed(
        &self,
        source_entity: Option<&dyn Entity>,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        old_signal: i32,
    ) {
        let signal = self.signal_strength(world.as_ref(), pos);
        self.base.check_pressed(
            source_entity,
            world,
            pos,
            old_signal,
            signal,
            Self::state_for_signal(state, signal),
            PRESSED_TIME,
            self.sound_click_on,
            self.sound_click_off,
        );
    }
}

impl BlockBehavior for WeightedPressurePlateBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BasePressurePlateBlock::can_survive(world, pos)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.base.state_for_placement(context)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        BasePressurePlateBlock::update_shape(state, world, pos, direction)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let signal = Self::signal_for_state(state);
        if signal > 0 {
            self.check_pressed(None, world, pos, state, signal);
        }
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        let signal = Self::signal_for_state(state);
        if signal == 0 {
            self.check_pressed(Some(entity), world, pos, state, signal);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.base.affect_neighbors_after_removal(
            world,
            pos,
            moved_by_piston,
            Self::signal_for_state(state),
        );
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        Self::signal_for_state(state)
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if direction == Direction::Up {
            Self::signal_for_state(state)
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_plate_uses_vanilla_float_ceiling_boundaries() {
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(0, 150), 0);
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(1, 150), 1);
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(10, 150), 1);
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(11, 150), 2);
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(150, 150), 15);
        assert_eq!(WeightedPressurePlateBlock::signal_for_count(200, 150), 15);
    }
}
