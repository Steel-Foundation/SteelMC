//! use to manage rewards

use crate::entity::{Entity, living_entity_loot_ref};
use crate::player::Player;
use steel_registry::advancement::AdvancementRewards;
use steel_registry::loot_table::LootContext;

/// Grants experience and loot rewards but recipes and functions are not handled yet
pub fn grant_reward(player: &Player, reward: &AdvancementRewards) {
    player.give_experience_points(reward.experience);

    let position = player.position();
    let mut rng = rand::rng();
    let mut ctx = LootContext::new(&mut rng)
        .with_this_entity(living_entity_loot_ref(player))
        .with_origin(position.x, position.y, position.z);

    let mut changes = false;
    for loot_table in &reward.loots {
        for mut item in loot_table.get_random_items(&mut ctx) {
            if player.add_item_with_sound(&mut item) {
                changes = true;
                continue;
            }
            if let Some(drop) = player.drop_item(item, false, false) {
                drop.set_no_pickup_delay();
                drop.set_owner(Some(player.gameprofile.id));
            }
        }
    }

    if changes {
        player.broadcast_inventory_changes();
    }

    //TODO grant recipes and functions when implemented
}
