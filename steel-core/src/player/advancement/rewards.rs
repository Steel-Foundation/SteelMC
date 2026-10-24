//! use to manage rewards

use crate::entity::{Entity, entity_loot_ref, living_loot_equipment};
use crate::loot::{LOOT_TABLES, LootContext};
use crate::player::Player;
use steel_registry::RegistryExt;
use steel_registry::advancement::AdvancementRewards;

/// Grants experience and loot rewards but recipes and functions are not handled yet
pub fn grant_reward(player: &Player, reward: &AdvancementRewards) {
    player.give_experience_points(reward.experience);

    if reward.loots.is_empty() {
        return;
    }
    let Some(world) = player.level() else {
        log::error!("Cannot grant advancement loot to a player without a world");
        return;
    };
    let position = player.position();
    let equipment = living_loot_equipment(player);

    let mut changes = false;
    for key in &reward.loots {
        let Some(loot_table) = LOOT_TABLES.by_key(key) else {
            log::error!("Advancement reward loot table {key} is not registered");
            continue;
        };
        let items = world.with_loot_random(loot_table, 0, |rng| {
            let mut ctx = LootContext::new(rng)
                .with_world(&world)
                .with_this_entity(entity_loot_ref(player, Some(&equipment)))
                .with_origin(position.x, position.y, position.z);
            loot_table.get_random_items(&mut ctx)
        });
        let items = match items {
            Ok(items) => items,
            Err(error) => {
                log::error!("Failed to generate advancement reward loot {key}: {error}");
                continue;
            }
        };
        for mut item in items {
            if player.add_item_with_sound(&mut item) {
                changes = true;
                continue;
            }
            if let Some(drop) = player.spawn_dropped_item(item, false, false) {
                drop.set_no_pickup_delay();
                drop.set_owner(Some(player.gameprofile.id()));
            }
        }
    }

    if changes {
        player.broadcast_inventory_changes();
    }

    //TODO grant recipes and functions when implemented
}
