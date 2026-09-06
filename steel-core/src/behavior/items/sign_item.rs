//! Sign item behavior implementation.
//!
//! Places sign blocks and opens the sign editor after placement.
//! Handles both standing signs (on ground) and wall signs (on walls).
//!
//! Wall and standing signs are handled by `StandingAndWallBlockItem`, not here.

use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_game_events;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};
use crate::entity::Entity;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader as _, World};

/// Behavior for hanging sign items that place hanging sign blocks.
///
/// Hanging signs can be placed as ceiling hanging signs or wall hanging signs.
#[item_behavior]
pub struct HangingSignItem {
    /// The ceiling hanging sign block.
    #[json_arg(vanilla_blocks, json = "block")]
    pub ceiling_block: BlockRef,
    /// The wall hanging sign block.
    #[json_arg(vanilla_blocks, json = "wall_block")]
    pub wall_block: BlockRef,
}

impl HangingSignItem {
    /// Creates a new hanging sign item behavior.
    #[must_use]
    pub const fn new(ceiling_block: BlockRef, wall_block: BlockRef) -> Self {
        Self {
            ceiling_block,
            wall_block,
        }
    }
}

/// Checks if a wall hanging sign can attach to a neighboring block.
///
/// This matches vanilla's `WallHangingSignBlock.canAttachTo`.
fn can_attach_to(
    world: &Arc<World>,
    sign_facing: Direction,
    attach_pos: BlockPos,
    attach_face: Direction,
) -> bool {
    let attach_state = world.get_block_state(attach_pos);
    let attach_block = REGISTRY.blocks.by_state_id(attach_state);

    if let Some(block) = attach_block
        && block.has_tag(&BlockTag::WALL_HANGING_SIGNS)
    {
        // Wall hanging signs can chain if they're on the same axis
        if let Some(neighbor_facing) =
            attach_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
        {
            return neighbor_facing.axis() == sign_facing.axis();
        }
    }

    // Otherwise, check for sturdy face with FULL support
    world.is_face_sturdy_for(attach_state, attach_pos, attach_face, SupportType::Full)
}

/// Checks if a wall hanging sign can be placed at the given position.
///
/// This matches vanilla's `WallHangingSignBlock.canPlace` which is called
/// from `HangingSignItem.canPlace` in addition to `canSurvive`.
fn can_wall_hanging_sign_place(world: &Arc<World>, state: BlockStateId, pos: BlockPos) -> bool {
    let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) else {
        return false;
    };

    let clockwise = facing.rotate_y_clockwise();
    let counter_clockwise = facing.rotate_y_counter_clockwise();

    let can_attach_clockwise = {
        let attach_pos = clockwise.relative(pos);
        can_attach_to(world, facing, attach_pos, counter_clockwise)
    };

    let can_attach_counter = {
        let attach_pos = counter_clockwise.relative(pos);
        can_attach_to(world, facing, attach_pos, clockwise)
    };

    can_attach_clockwise || can_attach_counter
}

/// Checks if a wall hanging sign block state can be placed.
///
/// This matches vanilla's `HangingSignItem.canPlace` override which adds
/// an additional check for `WallHangingSignBlock.canPlace`.
fn can_place_hanging_sign(world: &Arc<World>, state: BlockStateId, pos: BlockPos) -> bool {
    let block = REGISTRY.blocks.by_state_id(state);

    // If it's a wall hanging sign, we need the additional canPlace check
    if let Some(block) = block
        && block.has_tag(&BlockTag::WALL_HANGING_SIGNS)
        && !can_wall_hanging_sign_place(world, state, pos)
    {
        return false;
    }

    // All hanging signs need canSurvive check (handled by get_state_for_placement)
    true
}

impl ItemBehavior for HangingSignItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let has_infinite_materials = context.player.has_infinite_materials();
        let mut place_context = context.build_place_context();
        if !place_context.can_place() {
            return InteractionResult::Fail;
        }
        let place_pos = place_context.place_pos();

        let block_behaviors = &*BLOCK_BEHAVIORS;

        // Try ceiling hanging sign first if clicked from below, otherwise try wall
        let blocks_to_try = if context.hit_result.direction == Direction::Down {
            [self.ceiling_block, self.wall_block]
        } else {
            [self.wall_block, self.ceiling_block]
        };

        let mut new_state = None;
        let mut placed_block = None;
        for block in blocks_to_try {
            let behavior = block_behaviors.get_behavior(block);
            let Some(state) = behavior.get_state_for_placement(&place_context) else {
                continue;
            };

            // Vanilla's HangingSignItem.canPlace has additional check for wall hanging signs
            if !can_place_hanging_sign(context.world, state, place_pos) {
                continue;
            }

            let collision_shape = state.get_collision_shape_at(place_pos);
            if context.world.is_unobstructed(collision_shape, place_pos) {
                new_state = Some(state);
                placed_block = Some(block);
                break;
            }
        }

        let Some(state) = new_state else {
            return InteractionResult::Fail;
        };

        if !context
            .world
            .set_block(place_pos, state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        let placed_state = context.world.get_block_state(place_pos);
        let placed_behavior = BLOCK_BEHAVIORS.get_behavior(placed_state.get_block());
        placed_behavior.set_placed_by(
            placed_state,
            context.world,
            place_pos,
            place_context.source(),
        );

        if let Some(block) = placed_block {
            let sound_type = &block.config.sound_type;
            context.world.play_block_sound(
                sound_type.place_sound,
                place_pos,
                sound_type.volume,
                sound_type.pitch,
                Some(context.player.id()),
            );
        }
        context.world.game_event(
            &vanilla_game_events::BLOCK_PLACE,
            place_pos,
            &GameEventContext::new(Some(context.player), Some(placed_state)),
        );

        place_context.with_item_mut(|item| item.consume_one(has_infinite_materials));

        InteractionResult::Success
    }
}
