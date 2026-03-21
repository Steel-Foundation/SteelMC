//! Sign item behavior implementation.
//!
//! Places sign blocks and opens the sign editor after placement.
//! Handles both standing signs (on ground) and wall signs (on walls).
//!
//! **Vanilla reference:** `SignItem` extends `StandingAndWallBlockItem` and only
//! overrides `updateCustomBlockEntityTag` to open the sign editor after placement.

use std::sync::Arc;
use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::{TaggedRegistryExt, vanilla_block_tags};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use super::standing_and_wall_block_item::StandingAndWallBlockItem;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};
use crate::world::World;

/// Behavior for sign items that place sign blocks and open the editor.
///
/// In vanilla, `SignItem` extends `StandingAndWallBlockItem` and only overrides
/// `updateCustomBlockEntityTag` to open the sign editor after placement.
///
/// The `_standing_block`, `_wall_block`, and `_attachment_direction` fields are read by the
/// build script via `#[json_arg]` to generate constructor calls from `classes.json`.
/// The actual values are forwarded into `inner` — the fields themselves are not used at runtime.
#[item_behavior]
pub struct SignItem {
    #[json_arg(vanilla_blocks, json = "block")]
    _standing_block: BlockRef,
    #[json_arg(vanilla_blocks, json = "wall_block")]
    _wall_block: BlockRef,
    #[json_arg(
        r#enum = "Direction",
        module = "steel_registry::blocks::properties",
        json = "attachment_direction"
    )]
    _attachment_direction: Direction,
    /// Placement logic delegate (vanilla: `SignItem extends StandingAndWallBlockItem`).
    inner: StandingAndWallBlockItem,
}

impl SignItem {
    /// Creates a new sign item behavior for the given sign blocks.
    #[must_use]
    pub const fn new(
        standing_block: BlockRef,
        wall_block: BlockRef,
        attachment_direction: Direction,
    ) -> Self {
        Self {
            _standing_block: standing_block,
            _wall_block: wall_block,
            _attachment_direction: attachment_direction,
            inner: StandingAndWallBlockItem::new(standing_block, wall_block, attachment_direction),
        }
    }
}

impl ItemBehavior for SignItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let Some(place_context) = context.build_place_context() else {
            return InteractionResult::Fail;
        };
        let place_pos = place_context.relative_pos;

        let Some(new_state) = self.inner.get_placement_state(&place_context) else {
            return InteractionResult::Fail;
        };

        if !context
            .world
            .set_block(place_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        let block = self.inner.get_block_for_state(new_state);
        let sound_type = &block.config.sound_type;
        context.world.play_block_sound(
            sound_type.place_sound,
            place_pos,
            sound_type.volume,
            sound_type.pitch,
            Some(context.player.id),
        );

        context.inv.item().shrink(1);

        // Sign-specific: Open the sign editor for the player (front text by default)
        context.player.open_sign_editor(place_pos, true);

        InteractionResult::Success
    }
}

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
        && REGISTRY
            .blocks
            .is_in_tag(block, &vanilla_block_tags::WALL_HANGING_SIGNS_TAG)
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
    if let Some(b) = block
        && REGISTRY
            .blocks
            .is_in_tag(b, &vanilla_block_tags::WALL_HANGING_SIGNS_TAG)
        && !can_wall_hanging_sign_place(world, state, pos)
    {
        return false;
    }

    // All hanging signs need canSurvive check (handled by get_state_for_placement)
    true
}

impl ItemBehavior for HangingSignItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let Some(place_context) = context.build_place_context() else {
            return InteractionResult::Fail;
        };
        let place_pos = place_context.relative_pos;

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

            let collision_shape = state.get_collision_shape();
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

        context.inv.item().shrink(1);

        // Sign-specific: Open the sign editor for the player (front text by default)
        context.player.open_sign_editor(place_pos, true);

        InteractionResult::Success
    }
}
