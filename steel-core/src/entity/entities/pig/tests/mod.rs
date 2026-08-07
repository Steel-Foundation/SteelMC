use std::io::Cursor;
use std::string::ToString;

use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtTag;
use steel_registry::entity_type::EntityAttachment;
use steel_registry::init_vanilla_registry;
use steel_registry::{
    vanilla_blocks, vanilla_damage_types, vanilla_entities, vanilla_items,
    vanilla_pig_sound_variants, vanilla_pig_variants,
};
use steel_utils::UuidExt;
use uuid::Uuid;

use crate::entity::ai::navigation::NavigationTickContext;
use crate::entity::ai::node::Node;
use crate::entity::ai::path::{Path, PathType};
use crate::entity::damage::DamageSource;
use crate::entity::entities::LeashFenceKnotEntity;
use crate::entity::mob::LeashAttachment;
use crate::entity::{Animal, DEATH_DURATION, ItemSteerable, RemovalReason, SharedEntity};
use crate::inventory::equipment::EquipmentSlot;
use crate::test_support::{TestPlayerBuilder, fresh_test_world, test_world};
use crate::world::LevelReader;

use super::*;

struct EmptyNavigationLevel {
    air_state: BlockStateId,
}

impl EmptyNavigationLevel {
    fn new() -> Self {
        Self {
            air_state: REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR),
        }
    }
}

impl LevelReader for EmptyNavigationLevel {
    fn get_block_state(&self, _pos: BlockPos) -> BlockStateId {
        self.air_state
    }

    fn raw_brightness(&self, _pos: BlockPos, _sky_darkening: u8) -> u8 {
        0
    }

    fn min_y(&self) -> i32 {
        -64
    }

    fn height(&self) -> i32 {
        384
    }
}

mod ai_age;
mod animal_lifecycle;
mod core;
mod persistence;
