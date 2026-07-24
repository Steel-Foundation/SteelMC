//! Vanilla binary pressure-plate behavior.

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

const PRESSED_TIME: i32 = 20;

/// Vanilla `BlockSetType.PressurePlateSensitivity` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressurePlateSensitivity {
    /// Any non-spectating entity that responds to block triggers.
    Everything,
    /// Only entities implementing vanilla living-entity behavior.
    Mobs,
}

/// Vanilla on/off pressure plates, including wood and stone variants.
#[block_behavior]
pub struct PressurePlateBlock {
    base: BasePressurePlateBlock,
    #[json_arg(
        r#enum = "PressurePlateSensitivity",
        json = "type_pressure_plate_sensitivity"
    )]
    sensitivity: PressurePlateSensitivity,
    #[json_arg(sound_events, json = "type_pressure_plate_click_on")]
    sound_click_on: SoundEventRef,
    #[json_arg(sound_events, json = "type_pressure_plate_click_off")]
    sound_click_off: SoundEventRef,
}

impl PressurePlateBlock {
    /// Creates a binary pressure-plate behavior from extracted block-set data.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        sensitivity: PressurePlateSensitivity,
        sound_click_on: SoundEventRef,
        sound_click_off: SoundEventRef,
    ) -> Self {
        Self {
            base: BasePressurePlateBlock::new(block),
            sensitivity,
            sound_click_on,
            sound_click_off,
        }
    }

    fn signal_for_state(state: BlockStateId) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }

    fn state_for_signal(state: BlockStateId, signal: i32) -> BlockStateId {
        state.set_value(&BlockStateProperties::POWERED, signal > 0)
    }

    fn signal_strength(&self, world: &World, pos: BlockPos) -> i32 {
        let count =
            BasePressurePlateBlock::entity_count(world, pos, |entity| match self.sensitivity {
                PressurePlateSensitivity::Everything => true,
                // Class-hierarchy checks stay capability-based: raw fallback
                // entities become eligible when they gain `LivingEntity` behavior.
                PressurePlateSensitivity::Mobs => entity.is_living_entity(),
            });
        if count > 0 { 15 } else { 0 }
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

impl BlockBehavior for PressurePlateBlock {
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
    use steel_registry::{sound_events, test_support::init_test_registry, vanilla_blocks};

    use super::*;
    use crate::test_support::TestLevel;

    fn stone_pressure_plate() -> PressurePlateBlock {
        PressurePlateBlock::new(
            &vanilla_blocks::STONE_PRESSURE_PLATE,
            PressurePlateSensitivity::Mobs,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_ON,
            &sound_events::BLOCK_STONE_PRESSURE_PLATE_CLICK_OFF,
        )
    }

    #[test]
    fn pressure_plate_survives_on_rigid_or_center_support() {
        init_test_registry();
        let behavior = stone_pressure_plate();
        let pos = BlockPos::new(0, 64, 0);
        let state = vanilla_blocks::STONE_PRESSURE_PLATE.default_state();
        let rigid =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::STONE.default_state());
        let center =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::OAK_FENCE.default_state());

        assert!(behavior.can_survive(state, &rigid, pos));
        assert!(behavior.can_survive(state, &center, pos));
        assert!(!behavior.can_survive(state, &TestLevel::default(), pos));
    }

    #[test]
    fn powered_pressure_plate_strongly_powers_only_upward() {
        init_test_registry();
        let behavior = stone_pressure_plate();
        let state = vanilla_blocks::STONE_PRESSURE_PLATE
            .default_state()
            .set_value(&BlockStateProperties::POWERED, true);
        let level = TestLevel::default();

        assert_eq!(
            behavior.get_own_signal(state, &level, BlockPos::ZERO, SignalQueryContext::DEFAULT,),
            15
        );
        assert_eq!(
            behavior.get_direct_signal(
                state,
                &level,
                BlockPos::ZERO,
                Direction::Up,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
        assert_eq!(
            behavior.get_direct_signal(
                state,
                &level,
                BlockPos::ZERO,
                Direction::North,
                SignalQueryContext::DEFAULT,
            ),
            0
        );
    }
}
