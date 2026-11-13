use std::io::Cursor;
use std::sync::{Arc, Weak};

use glam::DVec3;
use simdnbt::borrow::read_compound as read_borrowed_compound;
use simdnbt::owned::NbtCompound;
use steel_registry::{init_vanilla_registry, vanilla_entities};
use steel_utils::{ChunkPos, Downcast as _};

use crate::entity::SharedEntity;
use crate::entity::{ENTITIES, Entity, LivingEntity, init_entities, reserve_entity_ids};
use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk, test_world};

use super::*;

/// Ids a dragon reserves: itself plus its eight parts.
const DRAGON_ID_BLOCK: u32 = 9;

/// Builds a dragon on a reserved ID block, as the registry does.
pub(super) fn build_dragon(position: DVec3, level: Weak<World>) -> Arc<EnderDragonEntity> {
    init_vanilla_registry();
    let ids = reserve_entity_ids(DRAGON_ID_BLOCK);
    Arc::new(EnderDragonEntity::new(
        &vanilla_entities::ENDER_DRAGON,
        ids.first(),
        position,
        level,
    ))
}

fn shared(dragon: &Arc<EnderDragonEntity>) -> SharedEntity {
    Arc::clone(dragon) as SharedEntity
}

/// A world-less dragon at the origin facing yaw 0 with a zeroed flight history, so the
/// part-offset tests can be exact.
fn test_dragon() -> Arc<EnderDragonEntity> {
    build_dragon(DVec3::ZERO, Weak::new())
}

fn dragon_at(world: &Arc<World>, position: DVec3) -> Arc<EnderDragonEntity> {
    build_dragon(position, Arc::downgrade(world))
}

/// A fresh world with full chunks out to `chunk_radius` around the origin: radius 0 for
/// the podium, 1 for a wall scan, 4 for the flight graph's radius-60 outer ring.
fn chunked_test_world(key: &'static str, chunk_radius: i32) -> TestWorld {
    init_vanilla_registry();
    let fixture = fresh_test_world(key);
    for x in -chunk_radius..=chunk_radius {
        for z in -chunk_radius..=chunk_radius {
            insert_ready_full_chunk(&fixture.world, ChunkPos::new(x, z));
        }
    }
    fixture
}

fn death_test_world(key: &'static str) -> TestWorld {
    chunked_test_world(key, 0)
}

/// `dragon`, switched into `phase`.
pub(super) fn in_phase(
    dragon: Arc<EnderDragonEntity>,
    phase: EnderDragonPhase,
) -> Arc<EnderDragonEntity> {
    dragon.phase_manager().set_phase(&dragon, phase);
    dragon
}

/// A dragon somewhere harmless inside the loaded chunk.
fn dragon_in_chunk(world: &Arc<World>) -> Arc<EnderDragonEntity> {
    dragon_at(world, DVec3::new(0.5, 80.0, 0.5))
}

fn tick_death_times(dragon: &Arc<EnderDragonEntity>, ticks: i32) {
    for _ in 0..ticks {
        dragon.tick_death(&shared(dragon));
    }
}

/// The dragon's head, its first sub-entity.
fn head(dragon: &EnderDragonEntity) -> &EnderDragonPart {
    dragon.sub_entities()[0]
        .downcast_ref::<EnderDragonPart>()
        .expect("the first sub-entity is the head")
}

mod core;
mod death;
mod flight_graph;
mod parts;
mod persistence;
