//! Ender eye item behavior implementation.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, types::UpdateFlags};

use crate::behavior::ItemBehavior;
use crate::behavior::block::push_entities_up;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::world::{LevelReader, World};

const END_PORTAL_PATTERN_DISTANCE: i32 = 5;
const END_PORTAL_PATTERN: [[char; 5]; 5] = [
    ['?', 'v', 'v', 'v', '?'],
    ['>', '?', '?', '?', '<'],
    ['>', '?', '?', '?', '<'],
    ['>', '?', '?', '?', '<'],
    ['?', '^', '^', '^', '?'],
];
const PATTERN_DIRECTIONS: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

/// Behavior for the ender eye item.
///
/// When used on an end portal frame without an eye, places the eye
/// and checks for portal completion.
#[item_behavior]
pub struct EnderEyeItem;

impl ItemBehavior for EnderEyeItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let clicked_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(clicked_pos);

        let Some(clicked_block) = REGISTRY.blocks.by_state_id(clicked_state) else {
            return InteractionResult::Pass;
        };

        if clicked_block.key != vanilla_blocks::END_PORTAL_FRAME.key {
            return InteractionResult::Pass;
        }

        let has_eye: bool = clicked_state.get_value(&BlockStateProperties::EYE);
        if has_eye {
            return InteractionResult::Pass;
        }

        let new_state = clicked_state.set_value(&BlockStateProperties::EYE, true);
        let new_state = push_entities_up(clicked_state, new_state, context.world, clicked_pos);

        if !context
            .world
            .set_block(clicked_pos, new_state, UpdateFlags::UPDATE_CLIENTS)
        {
            return InteractionResult::Pass;
        }
        context
            .world
            .update_neighbor_for_output_signal(clicked_pos, &vanilla_blocks::END_PORTAL_FRAME);

        // Play the end portal frame fill sound effect (no exclusion, all players hear it)
        context
            .world
            .level_event(level_events::END_PORTAL_FRAME_FILL, clicked_pos, 0, None);

        context.inv.with_item(|item| item.shrink(1));

        if let Some(portal_origin) = find_completed_end_portal_origin(context.world, clicked_pos) {
            spawn_end_portal(context.world, portal_origin);
        }

        InteractionResult::Success
    }
}

fn find_completed_end_portal_origin(
    level: &impl LevelReader,
    clicked_pos: BlockPos,
) -> Option<BlockPos> {
    for z in clicked_pos.z()..clicked_pos.z() + END_PORTAL_PATTERN_DISTANCE {
        for y in clicked_pos.y()..clicked_pos.y() + END_PORTAL_PATTERN_DISTANCE {
            for x in clicked_pos.x()..clicked_pos.x() + END_PORTAL_PATTERN_DISTANCE {
                let front_top_left = BlockPos::new(x, y, z);
                for forwards in PATTERN_DIRECTIONS {
                    for up in PATTERN_DIRECTIONS {
                        if up == forwards || up == forwards.opposite() {
                            continue;
                        }
                        if end_portal_pattern_matches(level, front_top_left, forwards, up) {
                            return Some(front_top_left.offset(-3, 0, -3));
                        }
                    }
                }
            }
        }
    }

    None
}

fn end_portal_pattern_matches(
    level: &impl LevelReader,
    front_top_left: BlockPos,
    forwards: Direction,
    up: Direction,
) -> bool {
    let forwards_vector = forwards.offset_vec();
    let up_vector = up.offset_vec();
    let right_vector = forwards_vector.cross(up_vector);

    for right in 0..5 {
        for down in 0..5 {
            let pattern_pos = BlockPos(front_top_left.0 + up_vector * -down + right_vector * right);
            if !end_portal_pattern_entry_matches(
                level,
                pattern_pos,
                END_PORTAL_PATTERN[down as usize][right as usize],
            ) {
                return false;
            }
        }
    }

    true
}

fn end_portal_pattern_entry_matches(
    level: &impl LevelReader,
    pos: BlockPos,
    pattern_entry: char,
) -> bool {
    match pattern_entry {
        '?' => true,
        '^' => end_portal_frame_matches(level, pos, Direction::South),
        '>' => end_portal_frame_matches(level, pos, Direction::West),
        'v' => end_portal_frame_matches(level, pos, Direction::North),
        '<' => end_portal_frame_matches(level, pos, Direction::East),
        _ => false,
    }
}

fn end_portal_frame_matches(level: &impl LevelReader, pos: BlockPos, facing: Direction) -> bool {
    let state = level.get_block_state(pos);
    state.get_block() == &vanilla_blocks::END_PORTAL_FRAME
        && state.get_value(&BlockStateProperties::EYE)
        && state.get_value(&BlockStateProperties::HORIZONTAL_FACING) == facing
}

fn spawn_end_portal(world: &Arc<World>, portal_origin: BlockPos) {
    let portal_state = vanilla_blocks::END_PORTAL.default_state();
    for x_offset in 0..3 {
        for z_offset in 0..3 {
            let portal_pos = portal_origin.offset(x_offset, 0, z_offset);
            let _ = world.destroy_block(portal_pos, true);
            let _ = world.set_block(portal_pos, portal_state, UpdateFlags::UPDATE_CLIENTS);
        }
    }

    world.global_level_event(
        level_events::SOUND_END_PORTAL_SPAWN,
        portal_origin.offset(1, 0, 1),
        0,
    );
}

#[cfg(test)]
mod tests {
    use steel_registry::blocks::block_state_ext::BlockStateExt;
    use steel_registry::blocks::properties::{BlockStateProperties, Direction};
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};
    use steel_utils::{BlockPos, BlockStateId};

    use crate::test_support::TestLevel;

    use super::find_completed_end_portal_origin;

    fn eye_frame(facing: Direction) -> BlockStateId {
        vanilla_blocks::END_PORTAL_FRAME
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing)
            .set_value(&BlockStateProperties::EYE, true)
    }

    fn place_inward_frame_ring(level: &TestLevel, origin: BlockPos) {
        for offset in 0..3 {
            level.set_test_block(origin.offset(offset, 0, -1), eye_frame(Direction::South));
            level.set_test_block(origin.offset(offset, 0, 3), eye_frame(Direction::North));
            level.set_test_block(origin.offset(-1, 0, offset), eye_frame(Direction::East));
            level.set_test_block(origin.offset(3, 0, offset), eye_frame(Direction::West));
        }
    }

    #[test]
    fn end_portal_pattern_matches_player_built_inward_layout() {
        init_test_registry();

        let level = TestLevel::default();
        let origin = BlockPos::new(4, 64, 9);
        place_inward_frame_ring(&level, origin);

        assert_eq!(
            find_completed_end_portal_origin(&level, origin.offset(1, 0, -1)),
            Some(origin)
        );
        assert_eq!(
            find_completed_end_portal_origin(&level, origin.offset(-1, 0, 2)),
            Some(origin)
        );
        assert_eq!(
            find_completed_end_portal_origin(&level, origin.offset(2, 0, 3)),
            Some(origin)
        );
        assert_eq!(
            find_completed_end_portal_origin(&level, origin.offset(3, 0, 0)),
            Some(origin)
        );
    }

    #[test]
    fn end_portal_pattern_rejects_wrong_side_facing() {
        init_test_registry();

        let level = TestLevel::default();
        let origin = BlockPos::new(4, 64, 9);
        place_inward_frame_ring(&level, origin);
        level.set_test_block(origin.offset(-1, 0, 1), eye_frame(Direction::West));

        assert_eq!(find_completed_end_portal_origin(&level, origin), None);
    }

    #[test]
    fn end_portal_pattern_uses_vanilla_front_top_left_offset() {
        init_test_registry();

        let level = TestLevel::default();
        let origin = BlockPos::new(4, 64, 9);
        place_inward_frame_ring(&level, origin);
        for offset in 0..3 {
            level.set_test_block(origin.offset(offset, 0, -1), eye_frame(Direction::North));
            level.set_test_block(origin.offset(offset, 0, 3), eye_frame(Direction::South));
        }

        assert_eq!(
            find_completed_end_portal_origin(&level, origin.offset(1, 0, -1)),
            Some(origin.offset(0, 0, -4))
        );
    }
}
