use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::{init_vanilla_registry, vanilla_entities};
use steel_utils::{ChunkPos, Downcast as _};

use crate::entity::{ENTITIES, Entity, LivingEntity, init_entities, reserve_entity_ids};
use crate::test_support::{fresh_test_world, insert_ready_full_chunk, test_world};

use super::*;

/// Ids a dragon reserves: itself plus its eight parts.
const DRAGON_ID_BLOCK: u32 = 9;

/// Builds a dragon on a properly reserved ID block, the way the registry does.
pub(super) fn build_dragon(position: DVec3, level: Weak<World>) -> EnderDragonEntity {
    init_vanilla_registry();
    let ids = reserve_entity_ids(DRAGON_ID_BLOCK);
    EnderDragonEntity::new(
        &vanilla_entities::ENDER_DRAGON,
        ids.first(),
        position,
        level,
    )
}

/// A world-less dragon at the origin facing yaw 0, with a zeroed flight history.
///
/// Every sample the part geometry reads is `(0, 0)` without any setup, which is what
/// lets the offset tests be exact rather than approximate.
fn test_dragon() -> EnderDragonEntity {
    build_dragon(DVec3::ZERO, Weak::new())
}

/// A dragon that knows its world, for the tests that tick it.
fn dragon_at(world: &Arc<World>, position: DVec3) -> EnderDragonEntity {
    build_dragon(position, Arc::downgrade(world))
}

/// A fresh world with full chunks out to `chunk_radius` around the origin.
///
/// Anything reading a heightmap needs real chunks, and how many depends on the reach
/// of what is being tested: one for the podium, three for a wall scan, nine for the
/// flight graph's radius-60 outer ring.
fn chunked_test_world(key: &'static str, chunk_radius: i32) -> Arc<World> {
    init_vanilla_registry();
    let world = fresh_test_world(key);
    for x in -chunk_radius..=chunk_radius {
        for z in -chunk_radius..=chunk_radius {
            insert_ready_full_chunk(&world, ChunkPos::new(x, z));
        }
    }
    world
}

/// A world holding just the podium chunk, which is all the death flight reads.
fn death_test_world(key: &'static str) -> Arc<World> {
    chunked_test_world(key, 0)
}

mod core;
mod death;
mod flight_graph;
mod parts;
mod persistence;
