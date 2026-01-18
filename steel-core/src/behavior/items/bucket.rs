//! Bucket item behavior implementations.

use std::ptr;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::items::ItemRef;
use steel_utils::types::UpdateFlags;

use crate::behavior::ItemBehavior;
use crate::behavior::context::{InteractionResult, UseOnContext};

/// Behavior for filled bucket items (water bucket, lava bucket, etc.)
///
/// When used on a block, places the fluid at the target position and
/// replaces the bucket with an empty bucket.
pub struct FilledBucketBehavior {
    /// The fluid block to place.
    fluid_block: BlockRef,
    /// The empty bucket item to give back.
    empty_bucket: ItemRef,
}

impl FilledBucketBehavior {
    /// Creates a new filled bucket behavior.
    #[must_use]
    pub const fn new(fluid_block: BlockRef, empty_bucket: ItemRef) -> Self {
        Self {
            fluid_block,
            empty_bucket,
        }
    }
}

impl ItemBehavior for FilledBucketBehavior {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        let clicked_pos = context.hit_result.block_pos;
        let clicked_state = context.world.get_block_state(&clicked_pos);
        let clicked_block = clicked_state.get_block();

        // Determine placement position
        // If the clicked block is replaceable, place there; otherwise place adjacent
        let place_pos = if clicked_block.config.replaceable {
            clicked_pos
        } else {
            context.hit_result.direction.relative(&clicked_pos)
        };

        // Check world bounds
        if !context.world.is_in_valid_bounds(&place_pos) {
            return InteractionResult::Fail;
        }

        // Check if we can place at this position
        let existing_state = context.world.get_block_state(&place_pos);
        let existing_block = existing_state.get_block();

        // Can only place in air or replaceable blocks
        // Also check that we're not placing water inside water (no-op)
        if !existing_block.config.replaceable {
            return InteractionResult::Fail;
        }

        // Don't place if it's already the same fluid
        if ptr::eq(existing_block, self.fluid_block) {
            return InteractionResult::Pass;
        }

        // Place the fluid block
        let fluid_state = self.fluid_block.default_state();
        if !context
            .world
            .set_block(place_pos, fluid_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
        {
            return InteractionResult::Fail;
        }

        // Replace the bucket with an empty bucket (unless in creative mode)
        if !context.player.has_infinite_materials() {
            context.item_stack.set_item(&self.empty_bucket.key);
        }

        // TODO: Play bucket empty sound

        InteractionResult::Success
    }
}
