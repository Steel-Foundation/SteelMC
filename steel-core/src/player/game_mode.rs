//! Game mode specific logic for player interactions.
//!
//! This module implements the logic from Java's `ServerPlayerGameMode`, particularly
//! the `useItemOn` method that handles block placement and block interactions.

use steel_registry::REGISTRY;
use steel_utils::types::{GameType, InteractionHand};

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockHitResult, ITEM_BEHAVIORS, InteractionResult, UseOnContext,
};
use crate::player::Player;
use crate::world::World;

/// Handles using an item on a block.
///
/// This implements the logic from Java's `ServerPlayerGameMode.useItemOn()`.
///
/// # Flow
/// 1. Spectator mode: Only allow opening menus (currently returns Pass)
/// 2. Check if block interaction should be suppressed (sneaking + holding items)
/// 3. If not suppressed: Call block's `use_item_on` method
/// 4. If block returns `TryEmptyHandInteraction` and main hand: Call block's `use_without_item`
/// 5. If item not empty: Call item behavior's `use_on` for placement
/// 6. Handle creative mode infinite materials
pub fn use_item_on(
    player: &Player,
    world: &World,
    hand: InteractionHand,
    hit_result: &BlockHitResult,
) -> InteractionResult {
    let pos = &hit_result.block_pos;
    let state = world.get_block_state(pos);

    // Spectator mode: can only open menus
    // TODO: Implement menu providers for blocks like chests
    if player.game_mode.load() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    // Check if block interaction should be suppressed (sneaking + holding items in either hand)
    let mut inv = player.inventory.lock();

    let have_something = !inv.get_item_in_hand(InteractionHand::MainHand).is_empty()
        || !inv.get_item_in_hand(InteractionHand::OffHand).is_empty();
    let suppress_block_use = player.is_secondary_use_active() && have_something;

    // Get behavior registries
    let block_behaviors = &*BLOCK_BEHAVIORS;
    let item_behaviors = &*ITEM_BEHAVIORS;

    // Try block interaction first (if not suppressed)
    if !suppress_block_use {
        // Get block behavior and call use_item_on
        let Some(block) = REGISTRY.blocks.by_state_id(state) else {
            // Block state not found in registry, skip block interaction
            return InteractionResult::Pass;
        };
        let behavior = block_behaviors.get_behavior(block);
        let item_stack = inv.get_item_in_hand(hand);

        let block_result =
            behavior.use_item_on(item_stack, state, world, *pos, player, hand, hit_result);

        if block_result.consumes_action() {
            return block_result;
        }

        // Try empty hand interaction for main hand if block requested it
        if matches!(block_result, InteractionResult::TryEmptyHandInteraction)
            && hand == InteractionHand::MainHand
        {
            // Release the inventory lock before calling use_without_item
            // since block behaviors may need to open menus
            drop(inv);

            let empty_result = behavior.use_without_item(state, world, *pos, player, hit_result);

            if empty_result.consumes_action() {
                return empty_result;
            }

            // Re-acquire lock for item use below
            inv = player.inventory.lock();
        }
    }

    // Try item use (block placement, etc.)
    let item_stack = inv.get_item_in_hand_mut(hand);
    if !item_stack.is_empty() {
        // TODO: Check item cooldowns
        // if player.getCooldowns().isOnCooldown(item_stack.item) { return Pass }

        let original_count = item_stack.count;

        let mut context = UseOnContext {
            player,
            hand,
            hit_result: hit_result.clone(),
            world,
            item_stack,
        };

        // Get item behavior and call use_on
        let item_behavior = item_behaviors.get_behavior(context.item_stack.item);
        let result = item_behavior.use_on(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() && context.item_stack.count < original_count {
            context.item_stack.count = original_count;
        }

        return result;
    }

    InteractionResult::Pass
}
