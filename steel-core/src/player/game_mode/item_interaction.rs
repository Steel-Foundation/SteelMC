use super::{
    Arc, BLOCK_BEHAVIORS, BlockHitResult, Entity, GameType, ITEM_BEHAVIORS, InteractionHand,
    InteractionResult, InventoryAccess, Player, REGISTRY, SUseItem, UseOnContext, World,
    adventure_mode, wrap_degrees,
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
        if !player.abilities.lock().may_build
            && !adventure_mode::can_place_on(&stack_before_use, world, pos)
        {
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

        // Restored in both directions: `use_on` can also grow the held stack when
        // its result merges back into the slot it came from.
        if player.has_infinite_materials() {
            context.inv.with_item(|item| item.count = original_count);
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
    let (is_empty, item_ref, stack_before_use) =
        inventory_access.with_item(|item| (item.is_empty(), item.item, item.clone()));

    if !is_empty {
        if player.is_item_on_cooldown(&stack_before_use) {
            return InteractionResult::Pass;
        }

        let mut context =
            crate::behavior::UseItemContext::new(player, hand, world, player.inventory.clone());

        // Get behavior registries
        let item_behaviors = &*ITEM_BEHAVIORS;
        let item_behavior = item_behaviors.get_behavior(item_ref);

        let is_instantly_used = item_behavior.get_use_duration(&stack_before_use, player) <= 0;

        let result = item_behavior.use_item(&mut context);

        if is_instantly_used && result.should_apply_item_use_side_effects() {
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

        self.reset_last_action_time();

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
        let target_component = |value: f32, current: f32| {
            if value.is_finite() {
                wrap_degrees(value)
            } else {
                current
            }
        };
        let target_rotation = (
            target_component(packet.y_rot, current_rotation.0),
            target_component(packet.x_rot, current_rotation.1),
        );
        if target_rotation != current_rotation {
            self.set_rotation(target_rotation);
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
    use std::sync::Arc;
    use super::use_item;
    use crate::behavior::{InteractionResult, init_behaviors};
    use crate::entity::Entity as _;
    use crate::player::connection::NetworkConnection as _;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};
    use glam::DVec3;
    use steel_protocol::packets::game::SUseItem;
    use steel_registry::{
        RegistryHolderSet,
        blocks::properties::Direction,
        data_component_predicate::DataComponentMatchers,
        data_components::{
            AdventureModePredicate, BlockPredicate, vanilla_components::CAN_PLACE_ON,
        },
        item_stack::ItemStack,
        items::item::BlockHitResult,
        vanilla_blocks, vanilla_items,
    };
    use steel_utils::{
        BlockPos, ChunkPos,
        types::{GameType, InteractionHand, UpdateFlags},
    };

    use super::use_item_on;

    #[test]
    fn use_item_discards_non_finite_rotation_components() {
        let world = fresh_test_world("use_item_non_finite_rotation");
        init_behaviors();
        let player = TestPlayerBuilder::new(world, "TestPlayer", 1).build();
        player.set_client_loaded(true);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::STICK));
        player.set_rotation((10.0, 20.0));

        for (sequence, y_rot, x_rot, expected) in [
            (1, f32::NAN, 30.0, (10.0, 30.0)),
            (2, 40.0, f32::INFINITY, (40.0, 30.0)),
            (3, f32::NEG_INFINITY, f32::NAN, (40.0, 30.0)),
        ] {
            player.handle_use_item(SUseItem {
                hand: InteractionHand::MainHand,
                sequence,
                y_rot,
                x_rot,
            });
            assert_eq!(player.rotation(), expected);
        }
        assert!(!player.connection.closed());
    }

    #[test]
    fn adventure_mode_requires_can_place_on_before_item_use() {
        let world = fresh_test_world("adventure_mode_can_place_on");
        init_behaviors();
        let pos = BlockPos::new(1, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_ALL,
        ));

        let player = TestPlayerBuilder::new(Arc::clone(&world), "TestPlayer", 1).build();
        player.restore_game_modes(GameType::Adventure, None);
        player
            .abilities
            .lock()
            .update_for_game_mode(GameType::Adventure);

        let hit_result = BlockHitResult {
            location: DVec3::new(1.5, 64.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };

        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::DIRT));
        assert!(
            !use_item_on(&player, &world, InteractionHand::MainHand, &hit_result).consumes_action()
        );
        assert_eq!(
            world.get_block_state(pos.above()),
            vanilla_blocks::AIR.default_state()
        );
        assert_eq!(player.inventory.lock().get_selected_item().count(), 1);

        let predicate = BlockPredicate::new(
            Some(RegistryHolderSet::Direct(vec![&vanilla_blocks::STONE])),
            None,
            None,
            DataComponentMatchers::ANY,
        );
        let can_place_on =
            AdventureModePredicate::new(vec![predicate]).expect("one block predicate is valid");
        let mut dirt = ItemStack::new(&vanilla_items::DIRT);
        dirt.set(CAN_PLACE_ON, can_place_on);
        player.inventory.lock().set_selected_item(dirt);

        assert!(
            use_item_on(&player, &world, InteractionHand::MainHand, &hit_result).consumes_action()
        );
        assert_eq!(
            world.get_block_state(pos.above()),
            vanilla_blocks::DIRT.default_state()
        );
        assert_eq!(player.inventory.lock().get_selected_item().count(), 0);
    }
  
    /// A player at full hunger cannot start eating a normal food item —
    /// vanilla `Consumable.canConsume` fails and returns `Fail` without
    /// starting active use.
    #[test]
    fn use_item_refuses_normal_food_at_full_hunger() {
        let world = fresh_test_world("use_item_full_hunger_normal_food");
        init_behaviors();
        let player = TestPlayerBuilder::new(world.clone(), "TestPlayer", 1).build();
        player.set_client_loaded(true);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::APPLE));

        let result = use_item(&player, &world, InteractionHand::MainHand);

        assert_eq!(result, InteractionResult::Fail);
        assert_eq!(player.active_item_use_hand(), None);
    }

    /// An always-edible food item (e.g. golden apple) can still be eaten at
    /// full hunger, matching vanilla `FoodProperties.canAlwaysEat`.
    #[test]
    fn use_item_allows_always_edible_food_at_full_hunger() {
        let world = fresh_test_world("use_item_full_hunger_always_edible_food");
        init_behaviors();
        let player = TestPlayerBuilder::new(world.clone(), "TestPlayer", 1).build();
        player.set_client_loaded(true);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::GOLDEN_APPLE));

        let result = use_item(&player, &world, InteractionHand::MainHand);

        assert_eq!(result, InteractionResult::Consume);
        assert_eq!(
            player.active_item_use_hand(),
            Some(InteractionHand::MainHand)
        );
    }
}
