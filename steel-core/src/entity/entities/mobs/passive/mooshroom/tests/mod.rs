use crate::entity::living_entity::shearing_loot_items_with_rng;
use std::sync::{Arc, Weak};
use steel_utils::random::{RandomSource, legacy_random::LegacyRandom};

use glam::DVec3;
use steel_registry::{init_vanilla_registry, vanilla_entities};
use steel_utils::types::InteractionHand;

use crate::bootstrap::init_globals;
use crate::entity::entities::CowEntity;
use crate::player::{Player, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world};

use super::*;

mod core;

fn mooshroom(id: i32) -> Arc<MushroomCowEntity> {
    MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, id, DVec3::ZERO, Weak::new())
}

fn player_holding(world: &Arc<World>, item: ItemStack) -> Arc<Player> {
    let player = TestPlayerBuilder::new(Arc::clone(world), "Farmer", 900_000).build();
    player.inventory.lock().set_selected_item(item);
    player
}

fn held(player: &Player) -> ItemStack {
    player.inventory.lock().get_selected_item().clone()
}

#[test]
fn shearing_loot_matches_the_mooshroom_variant() {
    init_vanilla_registry();
    let mooshroom =
        MushroomCowEntity::new(&vanilla_entities::MOOSHROOM, 1, DVec3::ZERO, Weak::new());
    let shears = ItemStack::new(&vanilla_items::SHEARS);
    for (variant, expected) in [
        (MooshroomVariant::Red, &*vanilla_items::RED_MUSHROOM),
        (MooshroomVariant::Brown, &*vanilla_items::BROWN_MUSHROOM),
    ] {
        mooshroom.set_variant(variant);
        let mut random = RandomSource::Legacy(LegacyRandom::from_seed(1));
        let drops = shearing_loot_items_with_rng(
            &mooshroom.living(),
            &vanilla_loot_tables::SHEARING_MOOSHROOM,
            &shears,
            &mut random,
        )
        .expect("shearing loot");
        assert_eq!(drops.len(), 5);
        assert!(
            drops
                .iter()
                .all(|drop| drop.is(expected) && drop.count() == 1)
        );
    }
}
