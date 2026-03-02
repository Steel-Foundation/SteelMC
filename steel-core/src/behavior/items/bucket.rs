//! Bucket item behavior implementations.
//!
//! Handles water buckets, lava buckets, and empty buckets.
//! Based on vanilla Minecraft's `BucketItem`.
//!
// TODO: Add support for bucket stacks (count > 1) without deadlocks
// TODO: Play bucket sounds (fill/empty)
// TODO: Spawn particles
// TODO: Handle waterlogging

use std::ptr;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::block_state_ext::FluidReplaceableExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::items::ItemRef;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_items;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::math::Vector3;
use steel_utils::types::UpdateFlags;

use crate::behavior::ItemBehavior;
use crate::behavior::context::InteractionResult;
use crate::entity::Entity;
use crate::fluid::flowing::{get_fluid_state_from_block, is_lava_state, is_water_state};
use crate::player::Player;

/// Computes the start (eye position) and end positions for a raytrace.
fn get_ray_endpoints(player: &Player) -> (Vector3<f64>, Vector3<f64>) {
    let pos = player.position();
    let start_pos = Vector3::new(pos.x, player.get_eye_y(), pos.z);
    let (yaw, pitch) = player.rotation();
    let (yaw_rad, pitch_rad) = (f64::from(yaw.to_radians()), f64::from(pitch.to_radians()));
    let block_interaction_range = 4.5;
    let direction = Vector3::new(
        -yaw_rad.sin() * pitch_rad.cos() * block_interaction_range,
        -pitch_rad.sin() * block_interaction_range,
        pitch_rad.cos() * yaw_rad.cos() * block_interaction_range,
    );

    let end_pos = start_pos.add(&direction);
    (start_pos, end_pos)
}

/// Checks if a fluid block state is a source block (level == 0).
fn is_source_fluid(state: BlockStateId, block: BlockRef) -> bool {
    if !ptr::eq(block, vanilla_blocks::WATER) && !ptr::eq(block, vanilla_blocks::LAVA) {
        return false;
    }

    state.try_get_value(&BlockStateProperties::LEVEL) == Some(0)
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
    fn use_item(&self, context: &mut crate::behavior::UseItemContext) -> InteractionResult {
        // Raytrace to find target block
        let (start, end) = get_ray_endpoints(context.player);
        let (ray_block, ray_dir) = context.world.raytrace(start, end, |pos, world| {
            let state = world.get_block_state(pos);
            let block = state.get_block();
            // Pass through air and all fluids
            // Use precise checks or tag checks if possible, but for raytrace pass-through, exact block check is fine
            // or better, check if it's not buildable.
            // Vanilla passes through liquids.
            if ptr::eq(block, vanilla_blocks::AIR) {
                return false;
            }
            // Check fluid state for pass-through
            let fluid_state = get_fluid_state_from_block(state);
            if !fluid_state.is_empty() {
                return false;
            }
            true
        });

        log::info!("Raytrace result: block={:?}, direction={:?}", ray_block, ray_dir);

        let (Some(clicked_pos), Some(direction)) = (ray_block, ray_dir) else {
            return InteractionResult::Fail;
        };

        if !context.world.is_in_valid_bounds(&clicked_pos) {
            return InteractionResult::Fail;
        }

        log::info!("Clicked block position: {:?}, direction: {:?}", clicked_pos, direction);
        let clicked_state = context.world.get_block_state(&clicked_pos);

        // Define fluid placement logic as a closure to reuse for primary/secondary targets
        let mut try_place_fluid = |pos: BlockPos| -> Option<InteractionResult> {
            if !context.world.is_in_valid_bounds(&pos) {
                return None;
            }

            let state = context.world.get_block_state(&pos);
            let fluid_state = get_fluid_state_from_block(state);

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
                    context
                        .world
                        .schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, 5);

                    // Consume bucket
                    if !context.player.has_infinite_materials() {
                        context.item_stack.set_item(&self.empty_bucket.key);
                    }
                    return Some(InteractionResult::Success);
                }
            }

            log::info!("Attempting standard fluid placement at {:?} with state {:?} and fluid_state {:?}", pos, state, fluid_state);
            // 2. Try Standard Placement (Replaceable block)
            if state.can_be_replaced_by_fluid(self.fluid_block) {
                log::info!("Block at {:?} is replaceable by fluid", pos);
                // If same fluid already exists and is source, just consume bucket (parity)
                // Use FluidState check
                let is_same_fluid = if is_water_bucket {
                    is_water_state(fluid_state)
                } else {
                    is_lava_state(fluid_state)
                };

                if is_same_fluid && fluid_state.is_source() {
                    if !context.player.has_infinite_materials() {
                        context.item_stack.set_item(&self.empty_bucket.key);
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
                    let tick_delay = if is_water_bucket { 5 } else { 30 };

                    let fluid_ref = if is_water_bucket {
                        &vanilla_fluids::WATER
                    } else {
                        &vanilla_fluids::LAVA
                    };
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

                    if !context.player.has_infinite_materials() {
                        context.item_stack.set_item(&self.empty_bucket.key);
                    }
                    return Some(InteractionResult::Success);
                }
            }
            None
        };

        // Determine Primary Target
        // If clicked block is waterloggable and we have water, try clicked_pos first.
        // Otherwise default to relative pos.
        // Note: We check if it HAS the property, not if it's empty, to match vanilla preference for containers.
        // (If full, it fails placement logic and falls back).
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
/// NOTE: Stack support (count > 1) is not yet implemented to avoid deadlocks.
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
    fn use_item(&self, context: &mut crate::behavior::UseItemContext) -> InteractionResult {
        let (start, end) = get_ray_endpoints(context.player);

        // Raytrace: stop on source fluids
        let (hit_block, hit_dir) = context.world.raytrace(start, end, |pos, world| {
            let state = world.get_block_state(pos);
            let block = state.get_block();

            if ptr::eq(block, vanilla_blocks::AIR) {
                return false;
            }

            if ptr::eq(block, vanilla_blocks::WATER) || ptr::eq(block, vanilla_blocks::LAVA) {
                return is_source_fluid(state, block);
            }

            true
        });

        let (Some(hit_pos), Some(_)) = (hit_block, hit_dir) else {
            return InteractionResult::Fail;
        };

        let fluid_state = context.world.get_block_state(&hit_pos);
        let fluid_block = fluid_state.get_block();
        log::info!("Fluid block: {}", fluid_block.key);
        // Determine filled bucket type
        let (filled_bucket, waterloggable) = if ptr::eq(fluid_block, vanilla_blocks::WATER)
            && is_source_fluid(fluid_state, fluid_block)
        {
            (&vanilla_items::ITEMS.water_bucket, false)
        } else if ptr::eq(fluid_block, vanilla_blocks::LAVA)
            && is_source_fluid(fluid_state, fluid_block)
        {
            (&vanilla_items::ITEMS.lava_bucket, false)
        } else if fluid_state
            .try_get_value(&BlockStateProperties::WATERLOGGED)
            .is_some()
        {
            log::info!("Water bucket on waterlogged block");
            (&vanilla_items::ITEMS.water_bucket, true)
        } else {
            log::info!("Other bucket");
            return InteractionResult::Fail;
        };

        let tick_delay = if ptr::eq(fluid_block, vanilla_blocks::WATER) {
            5
        } else {
            30
        };

        if waterloggable {
            let new_state = fluid_state.set_value(&BlockStateProperties::WATERLOGGED, false);
            context
                .world
                .set_block(hit_pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
        } else {
            // Remove fluid
            let air_state = REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
            if !context
                .world
                .set_block(hit_pos, air_state, UpdateFlags::UPDATE_ALL_IMMEDIATE)
            {
                return InteractionResult::Fail;
            }
        }

        let fluid_ref = if ptr::eq(fluid_block, vanilla_blocks::WATER) {
            &vanilla_fluids::WATER
        } else {
            &vanilla_fluids::LAVA
        };

        // IMPORTANT: Schedule ticks for neighbors so infinite sources regenerate
        // TODO: Visual sync issue - client sees air before regenerated water
        // This might need force-sending block updates to the specific player
        for offset in [
            (0, 1, 0),
            (0, -1, 0),
            (1, 0, 0),
            (-1, 0, 0),
            (0, 0, 1),
            (0, 0, -1),
        ] {
            let neighbor = hit_pos.offset(offset.0, offset.1, offset.2);
            context
                .world
                .schedule_fluid_tick_default(neighbor, fluid_ref, tick_delay);
        }

        // Play bucket fill sound
        let sound_id = if ptr::eq(fluid_block, vanilla_blocks::WATER) {
            sound_events::ITEM_BUCKET_FILL
        } else {
            sound_events::ITEM_BUCKET_FILL_LAVA
        };
        context
            .world
            .play_block_sound(sound_id, hit_pos, 1.0, 1.0, None);

        // Give filled bucket
        if !context.player.has_infinite_materials() {
            context.item_stack.set_item(&filled_bucket.key);
        }

        InteractionResult::Success
    }
}
