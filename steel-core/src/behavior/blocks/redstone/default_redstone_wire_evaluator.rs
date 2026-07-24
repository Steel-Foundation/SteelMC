//! Vanilla's non-experimental redstone-wire evaluator.

use std::sync::Arc;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::world::{
    LevelReader, SignalQueryContext, World, get_best_neighbor_signal, is_redstone_conductor,
};

use super::java_hash::sort_small_map_positions;

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
        let block_signal =
            get_best_neighbor_signal(level, pos, SignalQueryContext::without_wire_signals());
        if block_signal == 15 {
            return block_signal;
        }
        block_signal.max(self.get_incoming_wire_signal(level, pos))
    }

    fn get_incoming_wire_signal(&self, level: &dyn LevelReader, pos: BlockPos) -> i32 {
        let mut wire_signal = 0;

        for direction in Direction::HORIZONTAL {
            let neighbor_pos = pos.relative(direction);
            let neighbor_state = level.get_block_state(neighbor_pos);
            wire_signal = wire_signal.max(self.get_wire_signal(neighbor_state));

            let above_pos = pos.above();
            if is_redstone_conductor(level, neighbor_state, neighbor_pos)
                && !is_redstone_conductor(level, level.get_block_state(above_pos), above_pos)
            {
                let above_neighbor_pos = neighbor_pos.above();
                wire_signal = wire_signal
                    .max(self.get_wire_signal(level.get_block_state(above_neighbor_pos)));
            } else if !is_redstone_conductor(level, neighbor_state, neighbor_pos) {
                let below_neighbor_pos = neighbor_pos.below();
                wire_signal = wire_signal
                    .max(self.get_wire_signal(level.get_block_state(below_neighbor_pos)));
            }
        }

        0.max(wire_signal - 1)
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
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

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
}
