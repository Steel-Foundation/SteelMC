//! Ender eye item behavior implementation.

use std::sync::Arc;

use crate::entity::Entity;
use crate::worldgen::generator::ChunkGenerator;
use steel_macros::item_behavior;
use steel_registry::TaggedRegistryExt;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::{
    REGISTRY, level_events, vanilla_blocks, vanilla_entities, vanilla_game_events,
    vanilla_structure_tags,
};
use steel_utils::{BlockPos, types::UpdateFlags};

use crate::behavior::ItemBehavior;
use crate::behavior::block::push_entities_up;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::entity::entities::EyeOfEnderEntity;
use crate::entity::{SharedEntity, next_entity_id};
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, World};

use glam::DVec3;

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

    // This function doesn't run if you right click a end portal frame :)
    fn use_item(&self, context: &mut crate::behavior::UseItemContext) -> InteractionResult {
        let world = context.world;
        let Some(structure_generator) = world
            .chunk_map
            .world_gen_context
            .generator
            .structure_generator()
        else {
            log::debug!("World generator not found");
            return InteractionResult::Consume;
        };

        let Some(structures) = REGISTRY
            .structures
            .get_tag(&vanilla_structure_tags::StructureTag::EYE_OF_ENDER_LOCATED)
        else {
            log::debug!("Can't find `EYE_OF_ENDER_LOCATED` tag");
            return InteractionResult::Consume;
        };

        let structure_keys = structures
            .iter()
            .map(|structure| structure.key.clone())
            .collect::<Vec<_>>();

        let Some(structure_locate_plan) =
            structure_generator.locate_plan_for_structures(&structure_keys)
        else {
            log::debug!("No structure found");
            return InteractionResult::Consume;
        };

        let strongholds = structure_locate_plan.ring_candidates(context.player.block_position());

        let Some(closest_stronghold) = strongholds.first() else {
            return InteractionResult::Consume;
        };

        let stronghold_pos = closest_stronghold.locate_pos;

        let player_pos = context.player.position();
        let spawn_pos = DVec3::new(
            player_pos.x,
            player_pos.y + f64::from(context.player.base().dimensions().height * 0.5),
            player_pos.z,
        );

        let target_pos = DVec3::new(
            f64::from(stronghold_pos.x()),
            f64::from(stronghold_pos.y()),
            f64::from(stronghold_pos.z()),
        );

        world.game_event_at(
            &vanilla_game_events::PROJECTILE_SHOOT,
            spawn_pos,
            &GameEventContext::new(Some(context.player), None),
        );

        let eye = EyeOfEnderEntity::new(
            &vanilla_entities::EYE_OF_ENDER,
            next_entity_id(),
            spawn_pos,
            Arc::downgrade(world),
        );

        eye.init_target_pos(target_pos);

        let entity: SharedEntity = Arc::new(eye);
        if let Err(error) = world.try_add_entity(entity) {
            log::debug!("failed to spawn eye of ender: {error}");
            return InteractionResult::Consume;
        }

        context.inv.with_item(|item| item.shrink(1));

        InteractionResult::SuccessServer

        // TODO implement statistics and advancment
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
    use steel_registry::{init_vanilla_registry, vanilla_blocks};
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
        init_vanilla_registry();

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
        init_vanilla_registry();

        let level = TestLevel::default();
        let origin = BlockPos::new(4, 64, 9);
        place_inward_frame_ring(&level, origin);
        level.set_test_block(origin.offset(-1, 0, 1), eye_frame(Direction::West));

        assert_eq!(find_completed_end_portal_origin(&level, origin), None);
    }

    #[test]
    fn end_portal_pattern_uses_vanilla_front_top_left_offset() {
        init_vanilla_registry();

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
