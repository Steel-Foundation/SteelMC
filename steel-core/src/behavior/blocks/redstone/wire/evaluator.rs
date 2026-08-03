//! Vanilla's non-experimental redstone-wire evaluator.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::world::{LevelReader, SignalQueryContext, World};

use crate::behavior::blocks::redstone::java_hash::sort_small_map_positions;

/// Persistent evaluator used by `RedStoneWireBlock` when redstone experiments are disabled.
pub(super) struct DefaultRedstoneWireEvaluator {
    wire_block: BlockRef,
}

impl DefaultRedstoneWireEvaluator {
    pub(super) const fn new(wire_block: BlockRef) -> Self {
        Self { wire_block }
    }

    pub(super) fn update_power_strength(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) {
        let target_strength = self.calculate_target_strength(world.as_ref(), pos);
        if i32::from(state.get_value(&BlockStateProperties::POWER)) == target_strength {
            return;
        }

        if world.get_block_state(pos) == state {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::POWER, target_strength as u8),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }

        for update_pos in java_hash_set_update_order(pos) {
            world.update_neighbors_at(update_pos, self.wire_block);
        }
    }

    fn calculate_target_strength(&self, level: &dyn LevelReader, pos: BlockPos) -> i32 {
        let context = SignalQueryContext::without_wire_signals();
        let center_state = level.get_block_state(pos);
        let skip_center_signal =
            center_state.is_air() || center_state.get_block() == self.wire_block;
        let below_pos = pos.below();
        let above_pos = pos.above();
        let below_state = level.get_block_state(below_pos);
        let above_state = level.get_block_state(above_pos);

        let (below_signal, _) = self.get_vertical_signal(
            level,
            below_pos,
            below_state,
            Direction::Down,
            context,
            skip_center_signal,
        );
        if below_signal == 15 {
            return 15;
        }

        let (above_signal, above_is_conductor) = self.get_vertical_signal(
            level,
            above_pos,
            above_state,
            Direction::Up,
            context,
            skip_center_signal,
        );
        let mut signal = below_signal.max(above_signal);
        if signal == 15 {
            return 15;
        }

        for direction in Direction::HORIZONTAL {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = level.get_block_state(neighbor_pos);
            if neighbor_state.get_block() == self.wire_block {
                signal = signal.max(self.get_wire_signal(neighbor_state) - 1);
            } else {
                let (neighbor_signal, neighbor_is_conductor) = self.get_non_wire_signal(
                    level,
                    neighbor_pos,
                    neighbor_state,
                    direction,
                    context,
                    skip_center_signal,
                );
                signal = signal.max(neighbor_signal);
                if signal == 15 {
                    return 15;
                }

                if signal < 14 {
                    if neighbor_is_conductor && !above_is_conductor {
                        let above_neighbor_pos = neighbor_pos.above();
                        signal = signal.max(
                            self.get_wire_signal(level.get_block_state(above_neighbor_pos)) - 1,
                        );
                    } else if !neighbor_is_conductor {
                        let below_neighbor_pos = neighbor_pos.below();
                        signal = signal.max(
                            self.get_wire_signal(level.get_block_state(below_neighbor_pos)) - 1,
                        );
                    }
                }
            }

            if signal == 15 {
                return 15;
            }
        }

        signal
    }

    fn get_vertical_signal(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        context: SignalQueryContext,
        skip_center_signal: bool,
    ) -> (i32, bool) {
        if state.get_block() == self.wire_block {
            return (0, false);
        }

        self.get_non_wire_signal(level, pos, state, direction, context, skip_center_signal)
    }

    fn get_non_wire_signal(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        state: BlockStateId,
        direction: Direction,
        context: SignalQueryContext,
        skip_center_signal: bool,
    ) -> (i32, bool) {
        if state.is_air() {
            return (0, false);
        }

        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let mut signal = behavior.get_signal(state, level, pos, direction, context);
        let is_conductor = behavior.is_redstone_conductor(state, level, pos);
        if is_conductor && signal < 15 {
            signal = signal.max(self.get_direct_signal_to_except(
                level,
                pos,
                direction.opposite(),
                context,
                skip_center_signal,
            ));
        }

        (signal, is_conductor)
    }

    fn get_direct_signal_to_except(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        skip_direction: Direction,
        context: SignalQueryContext,
        skip_center_signal: bool,
    ) -> i32 {
        let mut signal = 0;
        for direction in Direction::ALL {
            if direction == skip_direction && skip_center_signal {
                continue;
            }

            let neighbor_pos = pos.relative(direction);
            let neighbor_state = level.get_block_state(neighbor_pos);
            if neighbor_state.is_air() || neighbor_state.get_block() == self.wire_block {
                continue;
            }

            signal = signal.max(
                BLOCK_BEHAVIORS
                    .get_behavior(neighbor_state.get_block())
                    .get_direct_signal(neighbor_state, level, neighbor_pos, direction, context),
            );
            if signal == 15 {
                return 15;
            }
        }

        signal
    }

    fn get_wire_signal(&self, state: BlockStateId) -> i32 {
        if state.get_block() == self.wire_block {
            i32::from(state.get_value(&BlockStateProperties::POWER))
        } else {
            0
        }
    }
}

/// Returns the iteration order of the seven-entry `HashSet<BlockPos>` created by
/// vanilla's default evaluator.
///
/// Seven inserts keep Java `HashMap` at its initial 16 buckets. Iteration walks
/// buckets from low to high and retains insertion order within a collision chain.
/// The stable insertion sort below models exactly that behavior without relying on
/// Rust's unrelated hash-table implementation.
fn java_hash_set_update_order(pos: BlockPos) -> [BlockPos; 7] {
    let mut positions = [
        pos,
        pos.below(),
        pos.above(),
        pos.north(),
        pos.south(),
        pos.west(),
        pos.east(),
    ];

    sort_small_map_positions(&mut positions);

    positions
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestLevel, fresh_test_world, insert_ready_full_chunk};
    use crate::world::{get_best_neighbor_signal, is_redstone_conductor};

    struct CountingLevel {
        level: TestLevel,
        block_state_reads: Cell<usize>,
    }

    impl CountingLevel {
        fn new(level: TestLevel) -> Self {
            Self {
                level,
                block_state_reads: Cell::new(0),
            }
        }

        fn block_state_reads(&self) -> usize {
            self.block_state_reads.get()
        }
    }

    impl LevelReader for CountingLevel {
        fn get_block_state(&self, pos: BlockPos) -> BlockStateId {
            self.block_state_reads.set(self.block_state_reads.get() + 1);
            self.level.get_block_state(pos)
        }

        fn raw_brightness(&self, pos: BlockPos, sky_darkening: u8) -> u8 {
            self.level.raw_brightness(pos, sky_darkening)
        }

        fn min_y(&self) -> i32 {
            self.level.min_y()
        }

        fn height(&self) -> i32 {
            self.level.height()
        }
    }

    fn powered_wire(power: u8) -> BlockStateId {
        vanilla_blocks::REDSTONE_WIRE
            .default_state()
            .set_value(&BlockStateProperties::POWER, power)
    }

    fn calculate_target_strength_reference(level: &dyn LevelReader, pos: BlockPos) -> i32 {
        let block_signal =
            get_best_neighbor_signal(level, pos, SignalQueryContext::without_wire_signals());
        if block_signal == 15 {
            return 15;
        }

        let evaluator = DefaultRedstoneWireEvaluator::new(&vanilla_blocks::REDSTONE_WIRE);
        block_signal.max(incoming_wire_power_reference(&evaluator, level, pos))
    }

    fn incoming_wire_power_reference(
        evaluator: &DefaultRedstoneWireEvaluator,
        level: &dyn LevelReader,
        pos: BlockPos,
    ) -> i32 {
        let mut signal = 0;
        let above_pos = pos.above();
        let above_is_conductor =
            is_redstone_conductor(level, level.get_block_state(above_pos), above_pos);

        for direction in Direction::HORIZONTAL {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = level.get_block_state(neighbor_pos);
            signal = signal.max(evaluator.get_wire_signal(neighbor_state));

            if is_redstone_conductor(level, neighbor_state, neighbor_pos) && !above_is_conductor {
                signal = signal
                    .max(evaluator.get_wire_signal(level.get_block_state(neighbor_pos.above())));
            } else if !is_redstone_conductor(level, neighbor_state, neighbor_pos) {
                signal = signal
                    .max(evaluator.get_wire_signal(level.get_block_state(neighbor_pos.below())));
            }
        }

        signal.saturating_sub(1)
    }

    fn assert_matches_reference(level: &dyn LevelReader, pos: BlockPos) {
        let evaluator = DefaultRedstoneWireEvaluator::new(&vanilla_blocks::REDSTONE_WIRE);
        assert_eq!(
            evaluator.calculate_target_strength(level, pos),
            calculate_target_strength_reference(level, pos),
        );
    }

    fn expected_positions(pos: BlockPos, labels: [&str; 7]) -> [BlockPos; 7] {
        labels.map(|label| match label {
            "center" => pos,
            "down" => pos.below(),
            "up" => pos.above(),
            "north" => pos.north(),
            "south" => pos.south(),
            "west" => pos.west(),
            "east" => pos.east(),
            _ => panic!("invalid test direction label"),
        })
    }

    #[test]
    fn seven_position_order_matches_target_jdk_hash_set_fixtures() {
        let fixtures = [
            (
                BlockPos::new(0, 64, 0),
                ["center", "down", "south", "east", "up", "north", "west"],
            ),
            (
                BlockPos::new(1, 64, 0),
                ["up", "north", "west", "center", "down", "south", "east"],
            ),
            (
                BlockPos::new(15, 64, 0),
                ["down", "south", "east", "up", "north", "west", "center"],
            ),
            (
                BlockPos::new(16, 64, 0),
                ["center", "down", "south", "east", "up", "north", "west"],
            ),
            (
                BlockPos::new(-16, -64, 31),
                ["down", "south", "east", "up", "north", "west", "center"],
            ),
            (
                BlockPos::new(30_000_000, 319, -30_000_000),
                ["down", "south", "east", "center", "up", "north", "west"],
            ),
        ];

        for (pos, labels) in fixtures {
            assert_eq!(
                java_hash_set_update_order(pos),
                expected_positions(pos, labels)
            );
        }
    }

    #[test]
    fn incoming_wire_power_does_not_feed_back_through_signal_queries() {
        init_test_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let powered_neighbor = vanilla_blocks::REDSTONE_WIRE
            .default_state()
            .set_value(&BlockStateProperties::POWER, 15);
        let level = TestLevel::default().with_block(pos.east(), powered_neighbor);
        let evaluator = DefaultRedstoneWireEvaluator::new(&vanilla_blocks::REDSTONE_WIRE);

        assert_eq!(evaluator.calculate_target_strength(&level, pos), 14);
    }

    #[test]
    fn target_strength_matches_vanilla() {
        init_test_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let stone = vanilla_blocks::STONE.default_state();
        let redstone_block = vanilla_blocks::REDSTONE_BLOCK.default_state();

        let cases = [
            TestLevel::default(),
            TestLevel::default().with_block(pos.east(), redstone_block),
            TestLevel::default().with_block(pos.east(), powered_wire(15)),
            TestLevel::default()
                .with_block(pos.east(), stone)
                .with_block(pos.east().above(), powered_wire(9)),
            TestLevel::default()
                .with_block(pos.east(), vanilla_blocks::GLASS.default_state())
                .with_block(pos.east().below(), powered_wire(9)),
            TestLevel::default()
                .with_block(pos.above(), stone)
                .with_block(pos.east(), stone)
                .with_block(pos.east().above(), powered_wire(15)),
            TestLevel::default()
                .with_block(pos.east(), stone)
                .with_block(pos.east().east(), redstone_block),
            TestLevel::default()
                .with_block(pos.north(), redstone_block)
                .with_block(pos.east(), powered_wire(12)),
        ];

        for level in &cases {
            assert_matches_reference(level, pos);
        }
    }

    #[test]
    fn target_strength_uses_fewer_block_state_reads() {
        init_test_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default()
            .with_block(pos.east(), vanilla_blocks::STONE.default_state())
            .with_block(pos.east().above(), powered_wire(12))
            .with_block(pos.north(), powered_wire(10));
        let reference_level = CountingLevel::new(level);
        let optimized_level = CountingLevel::new(
            TestLevel::default()
                .with_block(pos.east(), vanilla_blocks::STONE.default_state())
                .with_block(pos.east().above(), powered_wire(12))
                .with_block(pos.north(), powered_wire(10)),
        );
        let evaluator = DefaultRedstoneWireEvaluator::new(&vanilla_blocks::REDSTONE_WIRE);

        let reference_signal = calculate_target_strength_reference(&reference_level, pos);
        let optimized_signal = evaluator.calculate_target_strength(&optimized_level, pos);

        assert_eq!(optimized_signal, reference_signal);
        assert!(
            optimized_level.block_state_reads() < reference_level.block_state_reads(),
            "optimized reads: {}, reference reads: {}",
            optimized_level.block_state_reads(),
            reference_level.block_state_reads(),
        );
    }

    #[test]
    fn live_world_wire_line_settles_after_source_toggle() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("wire_evaluator_source_toggle");
        let wire_start = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(wire_start));

        let source_pos = wire_start.west();
        assert!(world.set_block(
            source_pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        for offset in 0..4 {
            assert!(world.set_block(
                wire_start.offset(offset, -1, 0),
                vanilla_blocks::STONE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
            assert!(world.set_block(
                wire_start.offset(offset, 0, 0),
                vanilla_blocks::REDSTONE_WIRE.default_state(),
                UpdateFlags::UPDATE_NONE,
            ));
        }

        assert!(world.set_block(
            source_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));
        for (offset, power) in [15_u8, 14, 13, 12].into_iter().enumerate() {
            assert_eq!(
                world
                    .get_block_state(wire_start.offset(offset as i32, 0, 0))
                    .get_value(&BlockStateProperties::POWER),
                power,
            );
        }

        assert!(world.remove_block(source_pos, false));
        for offset in 0..4 {
            assert_eq!(
                world
                    .get_block_state(wire_start.offset(offset, 0, 0))
                    .get_value(&BlockStateProperties::POWER),
                0,
            );
        }
    }
}
