use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::{init_vanilla_registry, vanilla_entities};
use steel_utils::types::InteractionHand;

use crate::bootstrap::init_globals;
use crate::entity::entities::CowEntity;
use crate::player::{Player, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world};

use super::*;

mod core;

fn mooshroom(id: i32) -> MushroomCowEntity {
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
