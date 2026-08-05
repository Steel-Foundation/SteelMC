//! Vanilla redstone signal queries shared by live and test level readers.

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::LevelReader;
use crate::behavior::BLOCK_BEHAVIORS;

/// State carried through one synchronous redstone signal query.
///
/// Vanilla's default wire evaluator temporarily disables the singleton wire block's
/// signal output while it measures non-wire input. Steel carries that exclusion in
/// the query instead, avoiding mutable global behavior shared across worlds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalQueryContext {
    wire_signals_enabled: bool,
}

impl SignalQueryContext {
    pub(crate) const DEFAULT: Self = Self {
        wire_signals_enabled: true,
    };

    /// Returns whether redstone wire may emit signal during this query.
    #[must_use]
    pub const fn wire_signals_enabled(self) -> bool {
        self.wire_signals_enabled
    }

    /// Returns a query context that excludes redstone-wire output.
    ///
    /// Vanilla temporarily clears `RedStoneWireBlock.shouldSignal` while its
    /// default evaluator measures power supplied by non-wire neighbors.
    pub(crate) const fn without_wire_signals() -> Self {
        Self {
            wire_signals_enabled: false,
        }
    }
}

/// Read-only redstone signal queries matching vanilla `SignalGetter`.
pub trait SignalGetter: LevelReader {
    /// Returns whether `state` conducts direct power at this level position.
    fn is_redstone_conductor(&self, state: BlockStateId, pos: BlockPos) -> bool;

    /// Returns the direct signal emitted by the block at `pos` toward `direction`.
    fn get_direct_signal(&self, pos: BlockPos, direction: Direction) -> i32;

    /// Returns the strongest direct signal entering `pos` from its six neighbors.
    fn get_direct_signal_to(&self, pos: BlockPos) -> i32;

    /// Returns the side input used by vanilla diode blocks.
    fn get_control_input_signal(
        &self,
        pos: BlockPos,
        direction: Direction,
        only_diodes: bool,
    ) -> i32;

    /// Returns whether the block at `pos` supplies a signal toward `direction`.
    fn has_signal(&self, pos: BlockPos, direction: Direction) -> bool;

    /// Returns the signal supplied by the block at `pos` toward `direction`.
    fn get_signal(&self, pos: BlockPos, direction: Direction) -> i32;

    /// Returns the strongest signal at `pos`, including the block's own source value.
    fn get_best_own_or_neighbour_signal(&self, pos: BlockPos) -> i32;

    /// Returns whether any of the six neighbors supplies signal to `pos`.
    fn has_neighbor_signal(&self, pos: BlockPos) -> bool;

    /// Returns the strongest signal supplied to `pos` by its six neighbors.
    fn get_best_neighbor_signal(&self, pos: BlockPos) -> i32;
}

impl<T: LevelReader> SignalGetter for T {
    fn is_redstone_conductor(&self, state: BlockStateId, pos: BlockPos) -> bool {
        is_redstone_conductor(self, state, pos)
    }

    fn get_direct_signal(&self, pos: BlockPos, direction: Direction) -> i32 {
        get_direct_signal(self, pos, direction, SignalQueryContext::DEFAULT)
    }

    fn get_direct_signal_to(&self, pos: BlockPos) -> i32 {
        get_direct_signal_to(self, pos, SignalQueryContext::DEFAULT)
    }

    fn get_control_input_signal(
        &self,
        pos: BlockPos,
        direction: Direction,
        only_diodes: bool,
    ) -> i32 {
        get_control_input_signal(self, pos, direction, only_diodes)
    }

    fn has_signal(&self, pos: BlockPos, direction: Direction) -> bool {
        get_signal(self, pos, direction, SignalQueryContext::DEFAULT) > 0
    }

    fn get_signal(&self, pos: BlockPos, direction: Direction) -> i32 {
        get_signal(self, pos, direction, SignalQueryContext::DEFAULT)
    }

    fn get_best_own_or_neighbour_signal(&self, pos: BlockPos) -> i32 {
        get_best_own_or_neighbour_signal(self, pos, SignalQueryContext::DEFAULT)
    }

    fn has_neighbor_signal(&self, pos: BlockPos) -> bool {
        has_neighbor_signal(self, pos, SignalQueryContext::DEFAULT)
    }

    fn get_best_neighbor_signal(&self, pos: BlockPos) -> i32 {
        get_best_neighbor_signal(self, pos, SignalQueryContext::DEFAULT)
    }
}

pub(crate) fn is_redstone_conductor(
    level: &dyn LevelReader,
    state: BlockStateId,
    pos: BlockPos,
) -> bool {
    BLOCK_BEHAVIORS
        .get_behavior(state.get_block())
        .is_redstone_conductor(state, level, pos)
}

pub(crate) fn get_direct_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    direction: Direction,
    context: SignalQueryContext,
) -> i32 {
    let state = level.get_block_state(pos);
    BLOCK_BEHAVIORS
        .get_behavior(state.get_block())
        .get_direct_signal(state, level, pos, direction, context)
}

pub(crate) fn get_direct_signal_to(
    level: &dyn LevelReader,
    pos: BlockPos,
    context: SignalQueryContext,
) -> i32 {
    let mut result = 0;
    for direction in Direction::ALL {
        result = result.max(get_direct_signal(
            level,
            direction.relative(pos),
            direction,
            context,
        ));
        if result >= 15 {
            return result;
        }
    }
    result
}

pub(crate) fn get_control_input_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    direction: Direction,
    only_diodes: bool,
) -> i32 {
    let state = level.get_block_state(pos);
    let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
    if only_diodes {
        return if behavior.is_diode() {
            get_direct_signal(level, pos, direction, SignalQueryContext::DEFAULT)
        } else {
            0
        };
    }
    if state.get_block() == &vanilla_blocks::REDSTONE_BLOCK {
        return 15;
    }
    if state.get_block() == &vanilla_blocks::REDSTONE_WIRE {
        return i32::from(state.get_value(&BlockStateProperties::POWER));
    }
    if behavior.is_signal_source(state, SignalQueryContext::DEFAULT) {
        get_direct_signal(level, pos, direction, SignalQueryContext::DEFAULT)
    } else {
        0
    }
}

pub(crate) fn get_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    direction: Direction,
    context: SignalQueryContext,
) -> i32 {
    let state = level.get_block_state(pos);
    let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
    let signal = behavior.get_signal(state, level, pos, direction, context);
    if behavior.is_redstone_conductor(state, level, pos) {
        signal.max(get_direct_signal_to(level, pos, context))
    } else {
        signal
    }
}

pub(crate) fn get_best_own_or_neighbour_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    context: SignalQueryContext,
) -> i32 {
    let state = level.get_block_state(pos);
    let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
    let own_signal = if behavior.is_signal_source(state, context) {
        behavior.get_own_signal(state, level, pos, context)
    } else {
        0
    };
    get_best_neighbor_signal(level, pos, context).max(own_signal)
}

pub(crate) fn has_neighbor_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    context: SignalQueryContext,
) -> bool {
    Direction::ALL
        .into_iter()
        .any(|direction| get_signal(level, direction.relative(pos), direction, context) > 0)
}

pub(crate) fn get_best_neighbor_signal(
    level: &dyn LevelReader,
    pos: BlockPos,
    context: SignalQueryContext,
) -> i32 {
    let mut best = 0;
    for direction in Direction::ALL {
        let signal = get_signal(level, direction.relative(pos), direction, context);
        if signal >= 15 {
            return 15;
        }
        best = best.max(signal);
    }
    best
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::properties::{AttachFace, BlockStateProperties};
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;
    use crate::behavior::init_behaviors;

    struct SignalTestLevel {
        states: Vec<(BlockPos, BlockStateId)>,
    }

    impl SignalTestLevel {
        fn new(states: Vec<(BlockPos, BlockStateId)>) -> Self {
            Self { states }
        }
    }

    impl LevelReader for SignalTestLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.states
                .iter()
                .find_map(|(state_pos, state)| (*state_pos == pos).then_some(*state))
                .unwrap_or_else(|| vanilla_blocks::AIR.default_state())
        }

        fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
            0
        }

        fn min_y(&self) -> i32 {
            -64
        }

        fn height(&self) -> i32 {
            384
        }
    }

    #[test]
    fn powered_button_directly_powers_its_support_block() {
        init_test_registry();
        init_behaviors();
        let target = BlockPos::new(4, 64, -3);
        let button = vanilla_blocks::STONE_BUTTON
            .default_state()
            .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Ceiling)
            .set_value(&BlockStateProperties::POWERED, true);
        let level = SignalTestLevel::new(vec![
            (target, vanilla_blocks::STONE.default_state()),
            (target.below(), button),
        ]);

        assert_eq!(level.get_direct_signal_to(target), 15);
        assert_eq!(level.get_signal(target, Direction::East), 15);
    }

    #[test]
    fn non_conductor_does_not_relay_direct_signal() {
        init_test_registry();
        init_behaviors();
        let target = BlockPos::new(4, 64, -3);
        let button = vanilla_blocks::STONE_BUTTON
            .default_state()
            .set_value(&BlockStateProperties::ATTACH_FACE, AttachFace::Ceiling)
            .set_value(&BlockStateProperties::POWERED, true);
        let level = SignalTestLevel::new(vec![
            (target, vanilla_blocks::GLASS.default_state()),
            (target.below(), button),
        ]);

        assert_eq!(level.get_direct_signal_to(target), 15);
        assert_eq!(level.get_signal(target, Direction::East), 0);
    }

    #[test]
    fn control_input_special_cases_redstone_block_and_wire() {
        init_test_registry();
        init_behaviors();
        let redstone_block_pos = BlockPos::new(0, 64, 0);
        let wire_pos = redstone_block_pos.east();
        let wire = vanilla_blocks::REDSTONE_WIRE
            .default_state()
            .set_value(&BlockStateProperties::POWER, 7_u8);
        let level = SignalTestLevel::new(vec![
            (
                redstone_block_pos,
                vanilla_blocks::REDSTONE_BLOCK.default_state(),
            ),
            (wire_pos, wire),
        ]);

        assert_eq!(
            level.get_control_input_signal(redstone_block_pos, Direction::North, false),
            15
        );
        assert_eq!(level.get_signal(redstone_block_pos, Direction::Down), 15);
        assert_eq!(
            level.get_best_own_or_neighbour_signal(redstone_block_pos),
            15
        );
        assert_eq!(
            level.get_control_input_signal(wire_pos, Direction::North, false),
            7
        );
        assert_eq!(
            level.get_control_input_signal(redstone_block_pos, Direction::North, true),
            0
        );
    }
}
