use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_registry::{init_vanilla_registry, vanilla_attributes, vanilla_entities};
use steel_utils::ChunkPos;
use steel_utils::types::InteractionHand;

use crate::behavior::{ITEM_BEHAVIORS, InteractionResult};
use crate::bootstrap::init_globals;
use crate::player::{Player, ResetReason};
use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

use super::*;

mod core;
mod shearing;

fn sheep(id: i32) -> SheepEntity {
    SheepEntity::new(&vanilla_entities::SHEEP, id, DVec3::ZERO, Weak::new())
}
