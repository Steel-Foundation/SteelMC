//! Standing and wall block item behavior implementation.
//!
//! This handles items like torches that place different block variants
//! depending on whether they're placed on top of a block (standing) or
//! on the side of a block (wall).
//!
//! **Vanilla differences:** None - this matches vanilla's `StandingAndWallBlockItem` exactly.
//! The placement logic iterates through `getNearestLookingDirections()` and tries each
//! direction (skipping the opposite of `attachmentDirection`), using the standing block
//! when direction matches `attachmentDirection` and wall block otherwise.

use steel_macros::item_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_utils::types::UpdateFlags;

use crate::behavior::context::{BlockPlaceContext, InteractionResult, UseOnContext};
use crate::behavior::{BLOCK_BEHAVIORS, ItemBehavior};

/// Behavior for items that place either a standing or wall variant of a block.
///
/// Used for torches (`torch/wall_torch`), soul torches, copper torches, etc.
/// When placed looking down (toward `attachmentDirection`), places the standing variant.
/// When placed looking horizontally or up, places the wall variant.
///
/// The `attachmentDirection` is typically `Direction::Down` for torches, meaning:
/// - Looking down → place standing torch on top of block below
/// - Looking horizontally → place wall torch on side of block
#[item_behavior(class = "StandingAndWallBlockItem")]
pub struct StandingAndWallBlockItem {
    /// The block to place when looking toward `attachmentDirection` (e.g., `torch`).
    #[json_arg(vanilla_blocks, json = "block")]
    pub standing_block: BlockRef,
    /// The block to place otherwise (e.g., `wall_torch`).
    #[json_arg(vanilla_blocks, json = "wall_block")]
    pub wall_block: BlockRef,
    /// The direction that triggers the standing block placement.
    /// For torches this is `Direction::Down` - when looking down, place standing torch.
    #[json_arg(
        r#enum = "Direction",
        module = "steel_registry::blocks::properties",
        json = "attachment_direction"
    )]
    pub attachment_direction: Direction,
}

impl StandingAndWallBlockItem {
    /// Creates a new standing and wall block item behavior.
    ///
    /// # Arguments
    /// * `standing_block` - Block placed when looking toward `attachment_direction`
    /// * `wall_block` - Block placed when looking away from `attachment_direction`
    /// * `attachment_direction` - Direction that triggers standing block (e.g., `Down` for torches)
    #[must_use]
    pub const fn new(
        standing_block: BlockRef,
        wall_block: BlockRef,
        attachment_direction: Direction,
    ) -> Self {
        Self {
            standing_block,
            wall_block,
            attachment_direction,
        }
    }

    /// Determines which block variant to use based on placement context.
    ///
    /// Vanilla caches the wall state once, then iterates through directions to decide
    /// between standing and wall variants. This matches `StandingAndWallBlockItem.getPlacementState`.
    #[must_use]
    pub fn get_placement_state(
        &self,
        place_context: &BlockPlaceContext<'_>,
    ) -> Option<steel_utils::BlockStateId> {
        let block_behaviors = &*BLOCK_BEHAVIORS;

        // Cache wall state once (vanilla does this before the loop)
        let wall_state = block_behaviors
            .get_behavior(self.wall_block)
            .get_state_for_placement(place_context);

        let directions = place_context.get_nearest_looking_directions();
        let skip_direction = self.attachment_direction.opposite();

        for direction in directions {
            // Skip the opposite of attachment direction
            // (e.g., for torches with attachment_direction=Down, skip Up)
            if direction == skip_direction {
                continue;
            }

            // Choose state based on direction
            let possible_state = if direction == self.attachment_direction {
                // Try standing block
                block_behaviors
                    .get_behavior(self.standing_block)
                    .get_state_for_placement(place_context)
            } else {
                // Use cached wall state
                wall_state
            };

            let Some(state) = possible_state else {
                continue;
            };

            // Vanilla's canPlace checks canSurvive (already done in get_state_for_placement)
            // Then checks isUnobstructed
            let collision_shape = state.get_collision_shape();
            if place_context
                .world
                .is_unobstructed(collision_shape, place_context.relative_pos)
            {
                return Some(state);
            }
        }

        None
    }

    /// Gets the block reference for a placed state (for sound lookup).
    #[must_use]
    pub fn get_block_for_state(&self, state: steel_utils::BlockStateId) -> BlockRef {
        // Determine which block was placed by checking the state
        if REGISTRY
            .blocks
            .by_state_id(state)
            .is_some_and(|b| b.key == self.standing_block.key)
        {
            self.standing_block
        } else {
            self.wall_block
        }
    }
}

impl ItemBehavior for StandingAndWallBlockItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let Some(place_context) = context.build_place_context() else {
            return InteractionResult::Fail;
        };
        let place_pos = place_context.relative_pos;

        let Some(new_state) = self.get_placement_state(&place_context) else {
            return InteractionResult::Fail;
        };

        if !context
            .world
            .set_block(place_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        let block = self.get_block_for_state(new_state);
        let sound_type = &block.config.sound_type;
        context.world.play_block_sound(
            sound_type.place_sound,
            place_pos,
            sound_type.volume,
            sound_type.pitch,
            Some(context.player.id),
        );

        context.item_stack.shrink(1);

        InteractionResult::Success
    }
}
