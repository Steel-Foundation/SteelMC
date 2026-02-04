//! Sign item behavior implementation.
//!
//! Places sign blocks and opens the sign editor after placement.
//! Handles both standing signs (on ground) and wall signs (on walls).
//!
//! **Vanilla reference:** `SignItem` extends `StandingAndWallBlockItem` and only
//! overrides `updateCustomBlockEntityTag` to open the sign editor after placement.

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use super::standing_and_wall_block_item::StandingAndWallBlockItem;
use crate::behavior::context::{BlockPlaceContext, InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};
use crate::world::World;

/// Behavior for sign items that place sign blocks and open the editor.
///
/// In vanilla, `SignItem` extends `StandingAndWallBlockItem` with `attachmentDirection = DOWN`.
/// We use composition here - wrapping `StandingAndWallBlockItem` and adding sign-specific
/// behavior (opening the sign editor after placement).
pub struct SignItemBehavior {
    /// The underlying standing and wall block item behavior.
    inner: StandingAndWallBlockItem,
}

impl SignItemBehavior {
    /// Creates a new sign item behavior for the given sign blocks.
    #[must_use]
    pub const fn new(standing_block: BlockRef, wall_block: BlockRef) -> Self {
        Self {
            inner: StandingAndWallBlockItem::new(standing_block, wall_block, Direction::Down),
        }
    }
}

impl ItemBehavior for SignItemBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let clicked_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(&clicked_pos);

        // Get the clicked block to check if it's replaceable
        let clicked_block = REGISTRY.blocks.by_state_id(clicked_state);
        let clicked_replaceable = clicked_block.is_some_and(|b| b.config.replaceable);

        // Determine placement position: replace clicked block if replaceable,
        // otherwise place adjacent to the clicked face
        let (place_pos, replace_clicked) = if clicked_replaceable {
            (clicked_pos, true)
        } else {
            (context.hit_result.direction.relative(&clicked_pos), false)
        };

        // Check if placement position is within world bounds
        if !context.world.is_in_valid_bounds(&place_pos) {
            return InteractionResult::Fail;
        }

        // Check if the placement position already has a non-replaceable block
        let existing_state = context.world.get_block_state(&place_pos);
        let existing_block = REGISTRY.blocks.by_state_id(existing_state);
        let existing_replaceable = existing_block.is_some_and(|b| b.config.replaceable);

        if !existing_replaceable {
            return InteractionResult::Fail;
        }

        // Get player rotation for placement context
        let (yaw, pitch) = context.player.rotation.load();

        let place_context = BlockPlaceContext {
            clicked_pos,
            clicked_face: context.hit_result.direction,
            click_location: context.hit_result.location,
            inside: context.hit_result.inside,
            relative_pos: place_pos,
            replace_clicked,
            horizontal_direction: Direction::from_yaw(yaw),
            rotation: yaw,
            pitch,
            world: context.world,
        };

        // Use StandingAndWallBlockItem's placement logic
        let Some(new_state) = self.inner.get_placement_state(&place_context) else {
            return InteractionResult::Fail;
        };

        // Place the block
        if !context
            .world
            .set_block(place_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        // Play place sound
        let block = self.inner.get_block_for_state(new_state);
        let sound_type = &block.config.sound_type;
        context.world.play_block_sound(
            sound_type.place_sound,
            place_pos,
            sound_type.volume,
            sound_type.pitch,
            Some(context.player.id),
        );

        // Consume one item from the stack
        context.item_stack.shrink(1);

        // Sign-specific: Open the sign editor for the player (front text by default)
        context.player.open_sign_editor(place_pos, true);

        InteractionResult::Success
    }
}

/// Behavior for hanging sign items that place hanging sign blocks.
///
/// Hanging signs can be placed as ceiling hanging signs or wall hanging signs.
pub struct HangingSignItemBehavior {
    /// The ceiling hanging sign block.
    pub ceiling_block: BlockRef,
    /// The wall hanging sign block.
    pub wall_block: BlockRef,
}

impl HangingSignItemBehavior {
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
    world: &World,
    sign_facing: Direction,
    attach_pos: &BlockPos,
    attach_face: Direction,
) -> bool {
    let attach_state = world.get_block_state(attach_pos);
    let attach_block = REGISTRY.blocks.by_state_id(attach_state);

    // Check if it's another wall hanging sign (vanilla uses BlockTags.WALL_HANGING_SIGNS)
    if let Some(block) = attach_block
        && block.key.path.contains("wall_hanging_sign")
    {
        // Wall hanging signs can chain if they're on the same axis
        if let Some(neighbor_facing) =
            attach_state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING)
        {
            return neighbor_facing.axis() == sign_facing.axis();
        }
    }

    // Otherwise, check for sturdy face with FULL support
    attach_state.is_face_sturdy_for(attach_face, SupportType::Full)
}

/// Checks if a wall hanging sign can be placed at the given position.
///
/// This matches vanilla's `WallHangingSignBlock.canPlace` which is called
/// from `HangingSignItem.canPlace` in addition to `canSurvive`.
fn can_wall_hanging_sign_place(world: &World, state: BlockStateId, pos: &BlockPos) -> bool {
    let Some(facing) = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING) else {
        return false;
    };

    let clockwise = facing.rotate_y_clockwise();
    let counter_clockwise = facing.rotate_y_counter_clockwise();

    let can_attach_clockwise = {
        let attach_pos = clockwise.relative(pos);
        can_attach_to(world, facing, &attach_pos, counter_clockwise)
    };

    let can_attach_counter = {
        let attach_pos = counter_clockwise.relative(pos);
        can_attach_to(world, facing, &attach_pos, clockwise)
    };

    can_attach_clockwise || can_attach_counter
}

/// Checks if a wall hanging sign block state can be placed.
///
/// This matches vanilla's `HangingSignItem.canPlace` override which adds
/// an additional check for `WallHangingSignBlock.canPlace`.
fn can_place_hanging_sign(world: &World, state: BlockStateId, pos: &BlockPos) -> bool {
    let block = REGISTRY.blocks.by_state_id(state);

    // If it's a wall hanging sign, we need the additional canPlace check
    if let Some(b) = block
        && b.key.path.contains("wall_hanging_sign")
        && !can_wall_hanging_sign_place(world, state, pos)
    {
        return false;
    }

    // All hanging signs need canSurvive check (handled by get_state_for_placement)
    true
}

impl ItemBehavior for HangingSignItemBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let clicked_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(&clicked_pos);

        // Get the clicked block to check if it's replaceable
        let clicked_block = REGISTRY.blocks.by_state_id(clicked_state);
        let clicked_replaceable = clicked_block.is_some_and(|b| b.config.replaceable);

        // Determine placement position
        let (place_pos, replace_clicked) = if clicked_replaceable {
            (clicked_pos, true)
        } else {
            (context.hit_result.direction.relative(&clicked_pos), false)
        };

        // Check if placement position is within world bounds
        if !context.world.is_in_valid_bounds(&place_pos) {
            return InteractionResult::Fail;
        }

        // Check if the placement position already has a non-replaceable block
        let existing_state = context.world.get_block_state(&place_pos);
        let existing_block = REGISTRY.blocks.by_state_id(existing_state);
        let existing_replaceable = existing_block.is_some_and(|b| b.config.replaceable);

        if !existing_replaceable {
            return InteractionResult::Fail;
        }

        // Get player rotation for placement context
        let (yaw, pitch) = context.player.rotation.load();

        let place_context = BlockPlaceContext {
            clicked_pos,
            clicked_face: context.hit_result.direction,
            click_location: context.hit_result.location,
            inside: context.hit_result.inside,
            relative_pos: place_pos,
            replace_clicked,
            horizontal_direction: Direction::from_yaw(yaw),
            rotation: yaw,
            pitch,
            world: context.world,
        };

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
            if let Some(state) = behavior.get_state_for_placement(&place_context) {
                // Vanilla's HangingSignItem.canPlace has additional check for wall hanging signs
                if !can_place_hanging_sign(context.world, state, &place_pos) {
                    continue;
                }

                let collision_shape = state.get_collision_shape();
                if context.world.is_unobstructed(collision_shape, &place_pos) {
                    new_state = Some(state);
                    placed_block = Some(block);
                    break;
                }
            }
        }

        let Some(state) = new_state else {
            return InteractionResult::Fail;
        };

        // Place the block
        if !context
            .world
            .set_block(place_pos, state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        // Play place sound
        if let Some(block) = placed_block {
            let sound_type = &block.config.sound_type;
            context.world.play_block_sound(
                sound_type.place_sound,
                place_pos,
                sound_type.volume,
                sound_type.pitch,
                Some(context.player.id),
            );
        }

        // Consume one item from the stack
        context.item_stack.shrink(1);

        // Open the sign editor for the player (front text by default)
        context.player.open_sign_editor(place_pos, true);

        InteractionResult::Success
    }
}
