use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_registry::item_stack::ItemStack;
use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_entities, vanilla_items};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, ChunkPos, Downcast as _};

use super::*;
use crate::entity::ai::path::PathType;
use crate::entity::{Animal, Entity, Mob, SharedEntity};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

mod core;
mod persistence;

/// Builds a turtle that is not attached to any world, for pure state tests.
fn detached_turtle() -> TurtleEntity {
    init_vanilla_registry();
    TurtleEntity::new(&vanilla_entities::TURTLE, 1, DVec3::ZERO, Weak::new())
}
