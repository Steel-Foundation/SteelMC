//! Bucket item behavior implementations.
//!
//! Handles water buckets, lava buckets, and empty buckets.
//! Based on vanilla Minecraft's `BucketItem`.
//!
// TODO: Add support for bucket stacks (count > 1) without deadlocks
// TODO: Spawn particles

use std::ptr;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::block_state_ext::FluidReplaceableExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_items;
use steel_utils::BlockPos;
use steel_utils::types::UpdateFlags;
use steel_utils::Direction;
use crate::behavior::context::InteractionResult;
use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS, ItemBehavior, UseItemContext};
use crate::fluid::{get_fluid_state_from_block, is_lava_state, is_water_state};
use crate::world::RaytraceAction;

/// Consumes one bucket from the player's hand, replacing it with `result_item`.
/// No-op if the player has infinite materials.
fn consume_bucket(context: &mut UseItemContext, result_item: ItemRef) {
    if context.player.has_infinite_materials() {
        return;
    }

    let result_stack = ItemStack::new(result_item);
    if context.item_stack.count() > 1 {
        context.item_stack.shrink(1);
        context.player.add_item_or_drop(result_stack);
    } else {
        context.item_stack.set_item(&result_item.key);
    }
}

/// Behavior for filled bucket items (water bucket, lava bucket)
///
/// Places fluid and gives back empty bucket.
/// NOTE: Stack support (count > 1) is not yet implemented to avoid deadlocks.
pub struct FilledBucketBehavior {
    fluid_block: BlockRef,
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
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        // Raytrace to find target block
        let (start, end) = context.player.get_ray_endpoints();
        let (ray_block, ray_dir) = context.world.raytrace(start, end, |pos, world| {
            let state = world.get_block_state(pos);
            let block = state.get_block();
            // Pass through air and all fluids
            if ptr::eq(block, vanilla_blocks::AIR) {
                return RaytraceAction::Pass;
            }
            // Check fluid state for pass-through
            let fluid_state = get_fluid_state_from_block(state);
            if !fluid_state.is_empty() {
                return RaytraceAction::Pass;
            }
            RaytraceAction::CheckShape
        });

        // If no block was hit, return fail
        let (Some(clicked_pos), Some(direction)) = (ray_block, ray_dir) else {
            return InteractionResult::Fail;
        };

        // If the block is out of bounds, return fail
        if !context.world.is_in_valid_bounds(&clicked_pos) {
            return InteractionResult::Fail;
        }

        let clicked_state = context.world.get_block_state(&clicked_pos);

        // Define fluid placement logic as a closure to reuse for primary/secondary targets
        let mut try_place_fluid = |pos: BlockPos| -> Option<InteractionResult> {
            if !context.world.is_in_valid_bounds(&pos) {
                return None;
            }

            let state = context.world.get_block_state(&pos);
            let fluid_state = get_fluid_state_from_block(state);

            // TODO: Nether water evaporation
            // If the dimension is THE_NETHER and we are placing WATER, we should not place the block.
            // Instead, we should play FIRE_EXTINGUISH sound, spawn LARGE_SMOKE particles, and empty the bucket.


            // TODO : THIS ISNT WORKING, WE NEED THAT WHEN WE SNEAK, WE PLACE THE FLUID ON THE NEIGHBOR
            // 1. Try Waterlogging (only if Water bucket)
            // Skipped if player is sneaking (parity with vanilla)
            let is_sneaking = context.player.is_shifting();
            // Determine if strict water bucket check - fluid_block is reliable for FilledBucket
            let is_water_bucket = ptr::eq(self.fluid_block, vanilla_blocks::WATER);

            if is_water_bucket
                && !is_sneaking
                && let Some(false) = state.try_get_value(&BlockStateProperties::WATERLOGGED)
            {
                let new_state = state.set_value(&BlockStateProperties::WATERLOGGED, true);
                if context
                    .world
                    .set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
                {
                    // Play bucket empty sound
                    context.world.play_block_sound(
                        sound_events::ITEM_BUCKET_EMPTY,
                        pos,
                        1.0,
                        1.0,
                        None,
                    );
                    // Schedule tick for fluid spread
                    let delay = FLUID_BEHAVIORS
                        .get_behavior(&vanilla_fluids::WATER)
                        .tick_delay(context.world);
                    context
                        .world
                        .schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);

                    // Consume bucket
                    consume_bucket(context, self.empty_bucket);
                    return Some(InteractionResult::Success);
                }
            }

            // 2. Try Standard Placement (Replaceable block)
            if state.can_be_replaced_by_fluid(self.fluid_block) {
                // If same fluid already exists and is source, just consume bucket (parity)
                // Use FluidState check
                let is_same_fluid = if is_water_bucket {
                    is_water_state(fluid_state)
                } else {
                    is_lava_state(fluid_state)
                };

                if is_same_fluid && fluid_state.is_source() {
                    if !context.player.has_infinite_materials() {
                        let empty_bucket =
                            steel_registry::item_stack::ItemStack::new(self.empty_bucket);
                        if context.item_stack.count() > 1 {
                            context.item_stack.shrink(1);
                            context.player.add_item_or_drop(empty_bucket);
                        } else {
                            context.item_stack.set_item(&self.empty_bucket.key);
                        }
                    }
                    return Some(InteractionResult::Success);
                }

                // Place fluid block
                let fluid_state_to_place = self.fluid_block.default_state();
                if context.world.set_block(
                    pos,
                    fluid_state_to_place,
                    UpdateFlags::UPDATE_ALL_IMMEDIATE,
                ) {
                    let fluid_ref = if is_water_bucket {
                        &vanilla_fluids::WATER
                    } else {
                        &vanilla_fluids::LAVA
                    };
                    let tick_delay = FLUID_BEHAVIORS
                        .get_behavior(fluid_ref)
                        .tick_delay(context.world);
                    context
                        .world
                        .schedule_fluid_tick_default(pos, fluid_ref, tick_delay);

                    let sound_id = if is_water_bucket {
                        sound_events::ITEM_BUCKET_EMPTY
                    } else {
                        sound_events::ITEM_BUCKET_EMPTY_LAVA
                    };
                    context
                        .world
                        .play_block_sound(sound_id, pos, 1.0, 1.0, None);

                    consume_bucket(context, self.empty_bucket);
                    return Some(InteractionResult::Success);
                }
            }
            None
        };

        // Determine Primary Target
        // If clicked block is waterloggable and we have water, try clicked_pos first.
        // Otherwise default to relative pos.
        let is_water_bucket = ptr::eq(self.fluid_block, vanilla_blocks::WATER);
        let clicked_is_waterloggable = clicked_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some();

        let primary_pos = if is_water_bucket && clicked_is_waterloggable {
            clicked_pos
        } else {
            direction.relative(&clicked_pos)
        };

        // Attempt Primary
        if let Some(result) = try_place_fluid(primary_pos) {
            return result;
        }

        // Attempt Secondary (Fallback)
        // If we started at clicked_pos and failed (e.g. full), try relative.
        if primary_pos == clicked_pos {
            let secondary_pos = direction.relative(&clicked_pos);
            if let Some(result) = try_place_fluid(secondary_pos) {
                return result;
            }
        }

        InteractionResult::Fail
    }
}

/// Behavior for empty bucket items.
///
/// Picks up fluid from source blocks and gives filled bucket.
pub struct EmptyBucketBehavior;

impl Default for EmptyBucketBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl EmptyBucketBehavior {
    /// Creates a new empty bucket behavior.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ItemBehavior for EmptyBucketBehavior {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let (start, end) = context.player.get_ray_endpoints();

        // Raytrace: stop on source fluids
        let (hit_block, hit_dir) = context.world.raytrace(start, end, |pos, world| {
            let state = world.get_block_state(pos);
            let block = state.get_block();

            if ptr::eq(block, vanilla_blocks::AIR) {
                return RaytraceAction::Pass;
            }

            let fluid_state = get_fluid_state_from_block(state);
            if fluid_state.is_source() {
                return RaytraceAction::ImmediateHit;
            }

            RaytraceAction::CheckShape
        });

        let Some(hit_pos) = hit_block else {
            return InteractionResult::Fail;
        };

        let fluid_state = context.world.get_block_state(&hit_pos);
        let block_behavior = BLOCK_BEHAVIORS.get_behavior(fluid_state.get_block());

        if let Some(result) =
            block_behavior.pickup_block(context.world, hit_pos, fluid_state, Some(context.player))
        {
            // Apply sound
            if let Some(sound) = result.sound {
                context
                    .world
                    .play_block_sound(sound, hit_pos, 1.0, 1.0, None);
            }

            // Give filled bucket
            consume_bucket(context, result.filled_bucket);

            let fluid_ref = get_fluid_state_from_block(fluid_state).fluid_id;

            // To be safe, if fluid_ref is empty, fallback to Water for tick scheduling
            let valid_fluid_ref = if fluid_ref.is_empty {
                &vanilla_fluids::WATER
            } else {
                fluid_ref
            };

            let tick_delay = FLUID_BEHAVIORS
                .get_behavior(valid_fluid_ref)
                .tick_delay(context.world);

            for direction in Direction::ALL {
                let neighbor = direction.relative(&hit_pos);
                context
                    .world
                    .schedule_fluid_tick_default(neighbor, valid_fluid_ref, tick_delay);
            }

            return InteractionResult::Success;
        }

        // Fallback for waterloggable blocks until they properly implement pickup_block
        if fluid_state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true) {
            let new_state = fluid_state.set_value(&BlockStateProperties::WATERLOGGED, false);
            context
                .world
                .set_block(hit_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);

            context
                .world
                .play_block_sound(sound_events::ITEM_BUCKET_FILL, hit_pos, 1.0, 1.0, None);

            consume_bucket(context, &vanilla_items::ITEMS.water_bucket);

            return InteractionResult::Success;
        }

        // Nothing was picked up — no fluid source block and no waterlogged block found.
        // Vanilla returns FAIL here so the client knows no item change occurred.
        InteractionResult::Fail
    }
}
