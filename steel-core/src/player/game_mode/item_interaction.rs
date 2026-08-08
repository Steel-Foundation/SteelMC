use super::{
    Arc, BLOCK_BEHAVIORS, BlockHitResult, Entity, GameType, ITEM_BEHAVIORS, InteractionHand,
    InteractionResult, InventoryAccess, Player, REGISTRY, SUseItem, UseOnContext, World,
    wrap_degrees,
};

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
    world: &Arc<World>,
    hand: InteractionHand,
    hit_result: &BlockHitResult,
) -> InteractionResult {
    let pos = hit_result.block_pos;
    let state = world.get_block_state(pos);

    // Spectator mode: can only open menus
    // TODO: Implement menu providers for blocks like chests
    if player.game_mode() == GameType::Spectator {
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

        let mut inventory_access = InventoryAccess::new(player.inventory.clone(), hand);

        let block_result = behavior.use_item_on(
            state,
            world,
            pos,
            player,
            hand,
            hit_result,
            &mut inventory_access,
        );

        if block_result.consumes_action() {
            return block_result;
        }

        if matches!(block_result, InteractionResult::TryEmptyHandInteraction)
            && hand == InteractionHand::MainHand
        {
            let empty_result = behavior.use_without_item(
                state,
                world,
                pos,
                player,
                hit_result,
                &mut inventory_access,
            );

            if empty_result.consumes_action() {
                return empty_result;
            }
        }
    }

    let inventory_access = InventoryAccess::new(player.inventory.clone(), hand);
    let (is_empty, original_count, item_ref, stack_before_use) =
        inventory_access.with_item(|item| (item.is_empty(), item.count, item.item, item.clone()));

    if !is_empty {
        if player.is_item_on_cooldown(&stack_before_use) {
            return InteractionResult::Pass;
        }

        let mut context = UseOnContext::new(
            player,
            hand,
            hit_result.clone(),
            world,
            player.inventory.clone(),
        );
        let item_behavior = item_behaviors.get_behavior(item_ref);
        let result = item_behavior.use_on(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() {
            context.inv.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        return result;
    }

    InteractionResult::Pass
}

/// Handles using an item (general usage like right-clicking air).
///
/// This implements logic similar to `ServerPlayerGameMode.useItem()`.
pub fn use_item(player: &Player, world: &Arc<World>, hand: InteractionHand) -> InteractionResult {
    // Spectator mode: can only open menus
    if player.game_mode() == GameType::Spectator {
        return InteractionResult::Pass;
    }

    let inventory_access = InventoryAccess::new(player.inventory.clone(), hand);
    let (is_empty, original_count, item_ref, stack_before_use) =
        inventory_access.with_item(|item| (item.is_empty(), item.count, item.item, item.clone()));

    if !is_empty {
        if player.is_item_on_cooldown(&stack_before_use) {
            return InteractionResult::Pass;
        }

        let mut context =
            crate::behavior::UseItemContext::new(player, hand, world, player.inventory.clone());

        // Get behavior registries
        let item_behaviors = &*ITEM_BEHAVIORS;
        let item_behavior = item_behaviors.get_behavior(item_ref);

        let result = item_behavior.use_item(&mut context);

        // Restore count for creative mode (infinite materials)
        if player.has_infinite_materials() {
            context.inv.with_item(|item| {
                if item.count < original_count {
                    item.count = original_count;
                }
            });
        }

        if result.should_apply_item_use_side_effects() {
            player.apply_item_use_cooldown(&stack_before_use);
        }

        return result;
    }

    InteractionResult::Pass
}

impl Player {
    /// Handles the use of an item.
    pub fn handle_use_item(&self, packet: SUseItem) {
        if !self.has_client_loaded() {
            return;
        }

        log::debug!(
            "Player {} used {:?} (sequence: {}, yaw: {}, pitch: {})",
            self.gameprofile.name,
            packet.hand,
            packet.sequence,
            packet.y_rot,
            packet.x_rot
        );

        self.ack_block_changes_up_to(packet.sequence);

        let item_stack_is_empty = {
            let inventory = self.inventory.lock();
            inventory.get_item_in_hand(packet.hand).is_empty()
        };
        if item_stack_is_empty {
            return;
        }

        let current_rotation = self.rotation();
        // Vanilla entity setters discard each non-finite rotation component independently.
        let target_yaw = if packet.y_rot.is_finite() {
            wrap_degrees(packet.y_rot)
        } else {
            current_rotation.0
        };
        let target_pitch = if packet.x_rot.is_finite() {
            wrap_degrees(packet.x_rot)
        } else {
            current_rotation.1
        };
        if current_rotation != (target_yaw, target_pitch) {
            self.set_rotation((target_yaw, target_pitch));
        }

        let world = self.get_world();
        let result = use_item(self, &world, packet.hand);

        if result.should_swing_server() {
            self.swing(packet.hand, true);
        }

        self.broadcast_inventory_changes();
    }
}

#[cfg(test)]
mod tests {
    use steel_protocol::packets::game::SUseItem;
    use steel_registry::{item_stack::ItemStack, vanilla_items};
    use steel_utils::types::InteractionHand;
    use uuid::Uuid;

    use crate::behavior::init_behaviors;
    use crate::entity::Entity as _;
    use crate::player::connection::NetworkConnection as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world};

    #[test]
    fn use_item_discards_non_finite_rotation_components() {
        let world = fresh_test_world("use_item_non_finite_rotation");
        init_behaviors();
        let player = TestPlayerBuilder::new(world, Uuid::from_u128(1), "TestPlayer", 1).build();
        player.set_client_loaded(true);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::STICK));
        player.set_rotation((10.0, 20.0));

        player.handle_use_item(SUseItem {
            hand: InteractionHand::MainHand,
            sequence: 1,
            y_rot: f32::NAN,
            x_rot: 30.0,
        });
        assert_eq!(player.rotation(), (10.0, 30.0));

        player.handle_use_item(SUseItem {
            hand: InteractionHand::MainHand,
            sequence: 2,
            y_rot: 40.0,
            x_rot: f32::INFINITY,
        });
        assert_eq!(player.rotation(), (40.0, 30.0));

        player.handle_use_item(SUseItem {
            hand: InteractionHand::MainHand,
            sequence: 3,
            y_rot: f32::NEG_INFINITY,
            x_rot: f32::NAN,
        });
        assert_eq!(player.rotation(), (40.0, 30.0));
        assert!(!player.connection.closed());
    }
}
