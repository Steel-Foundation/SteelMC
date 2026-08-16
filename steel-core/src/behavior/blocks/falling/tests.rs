use steel_registry::blocks::properties::Direction;
use steel_registry::{init_vanilla_registry, vanilla_blocks};
use steel_utils::BlockPos;

use super::FallingBlock;
use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
use crate::test_support::{ScheduledBlockTick, TestLevel};

#[test]
fn falling_shape_update_schedules_the_vanilla_two_tick_delay() {
    init_vanilla_registry();
    let level = TestLevel::default();
    let pos = BlockPos::new(3, 70, 4);
    let state = vanilla_blocks::SAND.default_state();
    let falling = FallingBlock::new(&vanilla_blocks::SAND);

    assert_eq!(falling.update_shape(state, &level, pos), state);
    assert_eq!(
        level.scheduled_block_ticks.borrow().as_slice(),
        [ScheduledBlockTick {
            pos,
            block: &vanilla_blocks::SAND,
            delay: 2,
        }]
    );
}

#[test]
fn concrete_powder_shape_update_matches_vanilla_liquid_adjacency() {
    init_vanilla_registry();
    init_behaviors();
    let pos = BlockPos::new(6, 70, 6);
    let powder_state = vanilla_blocks::WHITE_CONCRETE_POWDER.default_state();
    let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::WHITE_CONCRETE_POWDER);
    let side_water = TestLevel::default().with_block(
        pos.relative(Direction::East),
        vanilla_blocks::WATER.default_state(),
    );

    assert_eq!(
        behavior.update_shape(
            powder_state,
            &side_water,
            pos,
            Direction::East,
            pos.relative(Direction::East),
            vanilla_blocks::WATER.default_state(),
        ),
        vanilla_blocks::WHITE_CONCRETE.default_state()
    );
    assert!(side_water.scheduled_block_ticks.borrow().is_empty());

    // Vanilla deliberately does not solidify powder merely because water is
    // below it; the current powder state gates the downward adjacency check.
    let water_below = TestLevel::default().with_block(
        pos.relative(Direction::Down),
        vanilla_blocks::WATER.default_state(),
    );
    assert_eq!(
        behavior.update_shape(
            powder_state,
            &water_below,
            pos,
            Direction::Down,
            pos.relative(Direction::Down),
            vanilla_blocks::WATER.default_state(),
        ),
        powder_state
    );
    assert_eq!(water_below.scheduled_block_ticks.borrow()[0].delay, 2);
}
