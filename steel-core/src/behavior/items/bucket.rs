//! Bucket item behavior implementation.
//!
//! Handles water buckets, lava buckets, and empty buckets.
//!
//! Mirrors vanilla's `BucketItem(Fluid fluid)`: `fluid_block = None` = empty bucket,
//! `Some(block)` = filled bucket. Logic is dispatched in `use_item`.
//!
use crate::behavior::context::InteractionResult;
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockStateBehaviorExt, FLUID_BEHAVIORS, ItemBehavior, UseItemContext,
    pickup_waterlogged_block,
};
use crate::fluid::FluidStateExt;
use crate::inventory::lock::ContainerId;
use crate::world::RaytraceAction;
use steel_macros::item_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::fluid::FluidState;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::level_events;
use steel_registry::sound_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluids;
use steel_registry::vanilla_game_events;
use steel_registry::vanilla_items;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::world::game_event_context::GameEventContext;

/// Handles all bucket variants (empty, water, lava).
#[item_behavior]
pub struct BucketItem {
    #[json_arg(vanilla_blocks, json = "content", optional = "empty")]
    fluid_block: Option<BlockRef>,
}

impl BucketItem {
    /// Creates a new bucket behavior. `None` = empty bucket, `Some(block)` = filled.
    #[must_use]
    pub const fn new(fluid_block: Option<BlockRef>) -> Self {
        Self { fluid_block }
    }
}

impl ItemBehavior for BucketItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        match self.fluid_block {
            None => use_empty_bucket(context),
            Some(fluid_block) => use_filled_bucket(fluid_block, context),
        }
    }
}

/// Consumes one bucket from the player's hand, replacing it with `result_item`.
///
/// Vanilla parity: `ItemUtils.createFilledResult` with `limitCreativeStackSize = true`.
/// In creative mode the held stack is untouched, but the result item is added to the
/// inventory if the player doesn't already have one.
fn consume_bucket(context: &mut UseItemContext, result_item: ItemRef) {
    let player = context.player;
    context.inv.with_guard(|guard| {
        let inv_id = ContainerId::from_arc(&player.inventory);

        if player.has_infinite_materials() {
            // Creative: give the result item only if the player doesn't already have one.
            let already_has = guard.get(inv_id).is_some_and(|inv| {
                (0..inv.get_container_size()).any(|i| inv.get_item(i).item == result_item)
            });
            if !already_has {
                player.add_item_or_drop_with_guard(guard, ItemStack::new(result_item));
            }
            return;
        }

        let should_add_result = {
            let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
                return;
            };
            let hand_item = inv.get_item_in_hand_mut(context.hand);
            if hand_item.count() > 1 {
                hand_item.shrink(1);
                true
            } else {
                hand_item.set_item(&result_item.key);
                false
            }
        };

        if should_add_result {
            player.add_item_or_drop_with_guard(guard, ItemStack::new(result_item));
        }
    });
}

fn use_empty_bucket(context: &mut UseItemContext) -> InteractionResult {
    let (start, end) = context.player.get_ray_endpoints();

    // Raytrace: stop on source fluids
    let (hit_block, _) = context.world.raytrace(start, end, |pos, world| {
        let state = world.get_block_state(pos);
        let block = state.get_block();

        if block == &vanilla_blocks::AIR {
            return RaytraceAction::Pass;
        }

        let fluid_state = state.get_fluid_state();
        if fluid_state.is_source() {
            return RaytraceAction::ImmediateHit;
        }
        // Vanilla parity: ClipContext.Fluid.SOURCE_ONLY — flowing fluid is transparent.
        if !fluid_state.is_empty() {
            return RaytraceAction::Pass;
        }

        RaytraceAction::CheckShape
    });

    // Vanilla returns PASS when raytrace misses (allows other handlers to try)
    let Some(hit_pos) = hit_block else {
        return InteractionResult::Pass;
    };

    let hit_state = context.world.get_block_state(hit_pos);
    let block_behavior = BLOCK_BEHAVIORS.get_behavior(hit_state.get_block());

    if let Some(result) =
        block_behavior.pickup_block(context.world, hit_pos, hit_state, Some(context.player))
    {
        // Apply sound
        if let Some(sound) = result.sound {
            context
                .world
                .play_block_sound(sound, hit_pos, 1.0, 1.0, None);
        }

        // Give filled bucket
        consume_bucket(context, result.filled_bucket);
        context.world.game_event(
            &vanilla_game_events::FLUID_PICKUP,
            hit_pos,
            &GameEventContext::new(Some(context.player), None),
        );

        return InteractionResult::Success;
    }

    // TODO: Remove fallback once all waterloggable blocks implement pickup_block.
    if let Some(result) = pickup_waterlogged_block(
        block_behavior,
        context.world,
        hit_pos,
        hit_state,
        Some(context.player),
    ) {
        if let Some(sound) = result.sound {
            context
                .world
                .play_block_sound(sound, hit_pos, 1.0, 1.0, None);
        }

        consume_bucket(context, result.filled_bucket);
        context.world.game_event(
            &vanilla_game_events::FLUID_PICKUP,
            hit_pos,
            &GameEventContext::new(Some(context.player), None),
        );

        return InteractionResult::Success;
    }

    // Nothing was picked up — no fluid source block and no waterlogged block found.
    // Vanilla returns FAIL here so the client knows no item change occurred.
    InteractionResult::Fail
}

// TODO: Refactor into smaller helpers once all bucket types are implemented
#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's emptyContents flow; splitting would obscure the sequential placement logic"
)]
fn use_filled_bucket(fluid_block: BlockRef, context: &mut UseItemContext) -> InteractionResult {
    // Raytrace to find target block
    let (start, end) = context.player.get_ray_endpoints();
    let (ray_block, ray_dir) = context.world.raytrace(start, end, |pos, world| {
        let state = world.get_block_state(pos);
        let block = state.get_block();
        // Filled buckets use ClipContext.Fluid.NONE: ignore fluid shapes, but
        // still test the block shape of waterlogged/container blocks.
        if block == &vanilla_blocks::AIR {
            return RaytraceAction::Pass;
        }
        RaytraceAction::CheckShape
    });

    // Vanilla returns PASS when raytrace misses (allows other handlers to try)
    let (Some(clicked_pos), Some(direction)) = (ray_block, ray_dir) else {
        return InteractionResult::Pass;
    };

    // If the block is out of bounds, return fail
    if !context.world.is_in_valid_bounds(clicked_pos) {
        return InteractionResult::Fail;
    }

    let clicked_state = context.world.get_block_state(clicked_pos);
    let is_sneaking = context.player.is_crouching();

    // Define fluid placement logic as a closure to reuse for primary/secondary targets.
    // `check_sneak`: true for primary attempt, false for secondary (vanilla parity:
    // recursive emptyContents passes hitResult=null for fallback, bypassing sneak check).
    let mut try_place_fluid = |pos: BlockPos, check_sneak: bool| -> Option<InteractionResult> {
        if !context.world.is_in_valid_bounds(pos) {
            return None;
        }

        let state = context.world.get_block_state(pos);
        let fluid_state = state.get_fluid_state();

        // Vanilla parity (bl4): when sneaking, only air allows placement at this position.
        // Non-air blocks redirect to the neighbor — handled by the secondary call.
        // The secondary call bypasses this check (hitResult == null in vanilla).
        if check_sneak && is_sneaking && !state.get_block().config.is_air {
            return None;
        }

        let is_water_bucket = fluid_block == &vanilla_blocks::WATER;
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        let is_liquid_container = state.is_liquid_container();
        let can_place_liquid = is_water_bucket
            && is_liquid_container
            && behavior.can_place_liquid_with_player(
                state,
                FluidState::source(&vanilla_fluids::WATER).fluid_id,
                Some(context.player),
            );
        let can_replace = state.can_be_replaced_by_fluid(fluid_block);

        // Vanilla parity: block must be replaceable or liquid-container-admissible for placement.
        if !can_replace && !can_place_liquid {
            return None;
        }

        // Vanilla parity: in worlds where water evaporates (e.g. the Nether),
        // water buckets fizz out without placing any fluid.
        // TODO: Per-position environment attributes (vanilla uses EnvironmentAttributes.WATER_EVAPORATES per-pos)
        if is_water_bucket && context.world.dimension_type.water_evaporates {
            context
                .world
                .level_event(level_events::PARTICLES_WATER_EVAPORATING, pos, 0, None);
            consume_bucket(context, &vanilla_items::ITEMS.bucket);
            return Some(InteractionResult::Success);
        }

        // 1. Try LiquidBlockContainer handling (only if Water bucket).
        if is_water_bucket && is_liquid_container {
            let source_water = FluidState::source(&vanilla_fluids::WATER);
            behavior.place_liquid(context.world, pos, state, source_water);
            play_empty_sound_and_event(context, pos, true);
            consume_bucket(context, &vanilla_items::ITEMS.bucket);
            return Some(InteractionResult::Success);
        }

        // 2. Try Standard Placement (Replaceable block)
        if can_replace {
            // If same fluid already exists and is source, just consume bucket (parity)
            let is_same_fluid = if is_water_bucket {
                fluid_state.is_water()
            } else {
                fluid_state.is_lava()
            };

            if is_same_fluid && fluid_state.is_source() {
                play_empty_sound_and_event(context, pos, is_water_bucket);
                consume_bucket(context, &vanilla_items::ITEMS.bucket);
                return Some(InteractionResult::Success);
            }

            // Vanilla parity: destroy non-liquid replaceable blocks first so they
            // drop their items (e.g. tall grass, flowers, snow layers).
            if !state.get_block().config.liquid && !state.get_block().config.is_air {
                context.player.get_world().destroy_block(pos, true);
            }

            // Place fluid block
            let fluid_state_to_place = fluid_block.default_state();
            if context
                .world
                .set_block(pos, fluid_state_to_place, UpdateFlags::UPDATE_ALL_IMMEDIATE)
            {
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

                play_empty_sound_and_event(context, pos, is_water_bucket);

                consume_bucket(context, &vanilla_items::ITEMS.bucket);
                return Some(InteractionResult::Success);
            }
        }
        None
    };

    // Vanilla parity (BucketItem.java): position selection mirrors
    // `instanceof LiquidBlockContainer && content == Fluids.WATER ? pos : directionOffsetPos`.
    // If primary fails, secondary retries at the offset pos without sneak check,
    // matching vanilla's recursive `emptyContents(hitResult=null)` fallback.
    let is_water_bucket = fluid_block == &vanilla_blocks::WATER;
    let primary_pos =
        filled_bucket_primary_pos(clicked_state, clicked_pos, direction, is_water_bucket);

    // Attempt Primary (with sneak check)
    if let Some(result) = try_place_fluid(primary_pos, true) {
        return result;
    }

    // Attempt Secondary (Fallback — no sneak check, matching vanilla hitResult=null).
    // Vanilla's emptyContents always recurses with hitResult=null at the offset position
    // when the primary attempt fails, regardless of bucket type.
    let secondary_pos = direction.relative(clicked_pos);
    if let Some(result) = try_place_fluid(secondary_pos, false) {
        return result;
    }

    InteractionResult::Fail
}

fn play_empty_sound_and_event(context: &UseItemContext, pos: BlockPos, is_water_bucket: bool) {
    let sound_event = if is_water_bucket {
        &sound_events::ITEM_BUCKET_EMPTY
    } else {
        &sound_events::ITEM_BUCKET_EMPTY_LAVA
    };
    context
        .world
        .play_block_sound(sound_event, pos, 1.0, 1.0, None);
    context.world.game_event(
        &vanilla_game_events::FLUID_PLACE,
        pos,
        &GameEventContext::new(Some(context.player), None),
    );
}

fn filled_bucket_primary_pos(
    clicked_state: BlockStateId,
    clicked_pos: BlockPos,
    direction: Direction,
    is_water_bucket: bool,
) -> BlockPos {
    if is_water_bucket && clicked_state.is_liquid_container() {
        clicked_pos
    } else {
        direction.relative(clicked_pos)
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior::init_behaviors;
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use super::*;

    #[test]
    fn filled_water_bucket_targets_non_waterlogged_liquid_container_in_place() {
        init_test_registry();
        init_behaviors();

        let kelp = vanilla_blocks::KELP.default_state();

        assert_eq!(
            filled_bucket_primary_pos(kelp, BlockPos::ZERO, Direction::North, true),
            BlockPos::ZERO
        );
        assert_eq!(
            filled_bucket_primary_pos(kelp, BlockPos::ZERO, Direction::North, false),
            Direction::North.relative(BlockPos::ZERO)
        );
    }
}
