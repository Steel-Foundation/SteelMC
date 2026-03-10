//! Game mode specific logic for player interactions.
//!
//! This module implements the logic from Java's `ServerPlayerGameMode`, particularly
//! the `useItemOn` method that handles block placement and block interactions.

use steel_registry::REGISTRY;
use steel_utils::types::{GameType, InteractionHand};

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockHitResult, ITEM_BEHAVIORS, InteractionResult, UseOnContext,
};
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
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
    let have_something = {
        let inv = player.inventory.lock();
        !inv.get_item_in_hand(InteractionHand::MainHand).is_empty()
            || !inv.get_item_in_hand(InteractionHand::OffHand).is_empty()
    };

    let suppress_block_use = player.is_secondary_use_active() && have_something;

    // Get behavior registries
    let block_behaviors = &*BLOCK_BEHAVIORS;
    let item_behaviors = &*ITEM_BEHAVIORS;

    // Try block interaction first (if not suppressed).
    // No inventory lock held — block behaviors may need inventory access (e.g. opening chests).
    if !suppress_block_use {
        let Some(block) = REGISTRY.blocks.by_state_id(state) else {
            return InteractionResult::Pass;
        };
        let behavior = block_behaviors.get_behavior(block);

        // Brief lock for an immutable snapshot used during block interaction check
        let item_snapshot = player.inventory.lock().get_item_in_hand(hand).clone();

        let block_result =
            behavior.use_item_on(&item_snapshot, state, world, *pos, player, hand, hit_result);

        if block_result.consumes_action() {
            return block_result;
        }

        if matches!(block_result, InteractionResult::TryEmptyHandInteraction)
            && hand == InteractionHand::MainHand
        {
            let empty_result = behavior.use_without_item(state, world, *pos, player, hit_result);

            if empty_result.consumes_action() {
                return empty_result;
            }
        }
    }

    // Item use (block placement, etc.) — acquire inventory lock via ContainerLockGuard
    let inv_ref = ContainerRef::PlayerInventory(player.inventory.clone());
    let mut guard = ContainerLockGuard::lock_all(&[&inv_ref]);

    let inv_id = inv_ref.container_id();
    let mut item_stack = {
        let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
            return InteractionResult::Pass;
        };
        inv.get_item_in_hand(hand).clone()
    };

    if !item_stack.is_empty() {
        // TODO: Check item cooldowns
        // if player.getCooldowns().isOnCooldown(item_stack.item) { return Pass }

        let original_count = item_stack.count;

        let result = {
            let mut context = UseOnContext {
                player,
                hand,
                hit_result: hit_result.clone(),
                world,
                item_stack: &mut item_stack,
                inv_guard: &mut guard,
            };

            let item_behavior = item_behaviors.get_behavior(context.item_stack.item);
            let result = item_behavior.use_on(&mut context);

            // Restore count for creative mode (infinite materials)
            if player.has_infinite_materials() && context.item_stack.count < original_count {
                context.item_stack.count = original_count;
            }

            result
        };

        // Write back through the guard (context dropped, borrows released)
        if let Some(inv) = guard.get_player_inventory_mut(inv_id) {
            inv.set_item_in_hand(hand, item_stack);
        }

        return result;
    }

    InteractionResult::Pass
}

/// Handles using an item (general usage like right-clicking air).
///
/// This implements logic similar to `ServerPlayerGameMode.useItem()`.
pub fn use_item(player: &Player, world: &World, hand: InteractionHand) -> InteractionResult {
    // Spectator mode: can only open menus
    if player.game_mode.load() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    // TODO: Check item cooldowns
    // if player.getCooldowns().isOnCooldown(item_stack) { return InteractionResult::Pass }

    let inv_ref = ContainerRef::PlayerInventory(player.inventory.clone());
    let mut guard = ContainerLockGuard::lock_all(&[&inv_ref]);

    let mut item_stack = {
        let inv_id = inv_ref.container_id();
        let Some(inv) = guard.get_player_inventory_mut(inv_id) else {
            return InteractionResult::Pass;
        };
        inv.get_item_in_hand(hand).clone()
    };

    if !item_stack.is_empty() {
        let original_count = item_stack.count;

        let result = {
            let mut context = crate::behavior::UseItemContext {
                player,
                hand,
                world,
                item_stack: &mut item_stack,
                inv_guard: &mut guard,
            };

            // Get behavior registries
            let item_behaviors = &*ITEM_BEHAVIORS;
            let item_behavior = item_behaviors.get_behavior(context.item_stack.item);

            let result = item_behavior.use_item(&mut context);

            // Restore count for creative mode (infinite materials)
            if player.has_infinite_materials() && context.item_stack.count < original_count {
                context.item_stack.count = original_count;
            }

            result
        };

        // Write back modified hand item through the guard (context dropped, borrows released)
        let inv_id = inv_ref.container_id();
        if let Some(inv) = guard.get_player_inventory_mut(inv_id) {
            inv.set_item_in_hand(hand, item_stack);
        }

        return result;
    }

    InteractionResult::Pass
}
