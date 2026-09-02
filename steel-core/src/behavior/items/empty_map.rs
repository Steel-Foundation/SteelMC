//! Empty-map item behavior.

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{
    data_components::vanilla_components::MAP_ID, item_stack::ItemStack, sound_events, vanilla_items,
};

use crate::{
    behavior::{InteractionResult, ItemBehavior, UseItemContext},
    entity::Entity as _,
    inventory::container::Container as _,
    map::NewMapData,
};

/// Creates a fresh scale-zero filled map when used.
#[item_behavior]
pub struct EmptyMapItem;

impl ItemBehavior for EmptyMapItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let map_id = match context.world.map_data().create_map(NewMapData::blank(
            context.world,
            context.player.block_position(),
            0,
            true,
            false,
        )) {
            Ok(map_id) => map_id,
            Err(error) => {
                tracing::error!(
                    player = %context.player.gameprofile.name,
                    world = %context.world.key,
                    "could not allocate map data: {error}"
                );
                return InteractionResult::Fail;
            }
        };

        let mut map = ItemStack::new(&vanilla_items::FILLED_MAP);
        map.set(MAP_ID, map_id);
        let has_infinite_materials = context.player.has_infinite_materials();
        let overflow = context.inv.with_inventory(|inventory| {
            if !has_infinite_materials {
                inventory.shrink_item_in_hand(context.hand, 1);
            }
            if inventory.get_item_in_hand(context.hand).is_empty() {
                inventory.set_item_in_hand(context.hand, map);
                return ItemStack::empty();
            }

            let _ = inventory.add(&mut map);
            map
        });
        if !overflow.is_empty() && !has_infinite_materials {
            let _ = context.player.drop_item(overflow, false, false);
        }

        // TODO: Award `Stats.ITEM_USED` once Steel has a statistics foundation.
        context.world.play_sound_at(
            &sound_events::UI_CARTOGRAPHY_TABLE_TAKE_RESULT,
            SoundSource::Players,
            context.player.position(),
            1.0,
            1.0,
            None,
        );
        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use steel_registry::{
        data_components::vanilla_components::MAP_ID, item_stack::ItemStack,
        test_support::init_test_registry, vanilla_entities, vanilla_items,
    };
    use steel_utils::{
        WorldAabb,
        types::{GameType, InteractionHand},
    };
    use uuid::Uuid;

    use crate::{
        behavior::{InteractionResult, init_behaviors},
        inventory::container::Container,
        map::CarriedMap,
        player::{Player, ResetReason, game_mode::use_item},
        test_support::{TestPlayerBuilder, fresh_test_world},
        world::World,
    };

    fn test_player(world: &Arc<World>, uuid: u128, entity_id: i32) -> Arc<Player> {
        let player = TestPlayerBuilder::new(Arc::clone(world), "Cartographer", entity_id)
            .uuid(Uuid::from_u128(uuid))
            .build();
        assert!(world.add_player(Arc::clone(&player), ResetReason::InitialJoin));
        player
    }

    #[test]
    fn using_single_empty_map_creates_tracked_scale_zero_map() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("empty_map_single");
        let player = test_player(&world, 41, 41);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::MAP));

        assert_eq!(
            use_item(&player, &world, InteractionHand::MainHand),
            InteractionResult::Success
        );

        let (map_id, carried) = {
            let inventory = player.inventory.lock();
            let filled = inventory.get_selected_item();
            assert!(filled.is(&vanilla_items::FILLED_MAP));
            assert_eq!(filled.count(), 1);
            (
                filled
                    .get(MAP_ID)
                    .copied()
                    .expect("created map should carry its saved-data ID"),
                CarriedMap::from_item(filled).expect("created map should be trackable"),
            )
        };
        let packets = world
            .map_data()
            .synchronize_player(&player, &[carried], &[]);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].map_id, map_id);
        assert_eq!(packets[0].scale, 0);
        assert!(!packets[0].locked);
        let patch = packets[0]
            .color_patch
            .as_ref()
            .expect("a new map should send its blank color buffer");
        assert_eq!((patch.width, patch.height), (128, 128));
    }

    #[test]
    fn using_stacked_empty_maps_adds_filled_map_to_inventory() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("empty_map_stacked");
        let player = test_player(&world, 42, 42);
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::with_count(&vanilla_items::MAP, 2));

        assert_eq!(
            use_item(&player, &world, InteractionHand::MainHand),
            InteractionResult::Success
        );

        let inventory = player.inventory.lock();
        assert!(inventory.get_selected_item().is(&vanilla_items::MAP));
        assert_eq!(inventory.get_selected_item().count(), 1);
        let filled = inventory
            .items()
            .iter()
            .find(|item| item.is(&vanilla_items::FILLED_MAP))
            .expect("the filled map should be added to an inventory slot");
        assert_eq!(filled.count(), 1);
        assert!(filled.has(MAP_ID));
    }

    #[test]
    fn creative_full_inventory_discards_created_map_without_dropping_it() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("empty_map_creative_overflow");
        let player = test_player(&world, 43, 43);
        player.restore_game_modes(GameType::Creative, None);
        {
            let mut inventory = player.inventory.lock();
            for slot in &mut inventory.items_mut()[..36] {
                *slot = ItemStack::with_count(&vanilla_items::STONE, 64);
            }
            inventory.set_selected_item(ItemStack::new(&vanilla_items::MAP));
        }

        assert_eq!(
            use_item(&player, &world, InteractionHand::MainHand),
            InteractionResult::Success
        );

        let inventory = player.inventory.lock();
        assert!(inventory.get_selected_item().is(&vanilla_items::MAP));
        assert!(
            inventory
                .items()
                .iter()
                .all(|item| !item.is(&vanilla_items::FILLED_MAP))
        );
        drop(inventory);
        assert!(
            world
                .get_entities_in_aabb_matching(
                    &WorldAabb::new(-2.0, -1.0, -2.0, 2.0, 3.0, 2.0),
                    |entity| entity.entity_type() == &vanilla_entities::ITEM,
                )
                .is_empty()
        );
    }
}
