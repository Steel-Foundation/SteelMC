//! Fixtures shared by the vehicle tests.

use std::sync::Arc;

use glam::DVec3;
use steel_registry::vanilla_entities;
use steel_utils::ChunkPos;

use super::MinecartEntity;
use crate::bootstrap::init_globals;
use crate::entity::{EntityEventSource as _, next_entity_id};
use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
use crate::world::World;

pub(crate) fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-12,
        "expected {expected}, got {actual}",
    );
}

pub(crate) fn assert_vec_close(actual: DVec3, expected: DVec3) {
    assert_close(actual.x, expected.x);
    assert_close(actual.y, expected.y);
    assert_close(actual.z, expected.z);
}

/// A fresh test world with `chunk` loaded.
pub(crate) fn test_world(key: &'static str, chunk: ChunkPos) -> TestWorld {
    init_globals();

    let fixture = fresh_test_world(key);
    insert_ready_full_chunk(&fixture.world, chunk);
    fixture
}

/// A minecart added to `world` at `(8.5, 64, 8.5)`.
pub(crate) fn spawned_minecart(world: &Arc<World>) -> Arc<MinecartEntity> {
    let cart = Arc::new(MinecartEntity::new(
        &vanilla_entities::MINECART,
        next_entity_id(),
        DVec3::new(8.5, 64.0, 8.5),
        Arc::downgrade(world),
    ));

    world
        .try_add_entity(Arc::clone(&cart).into_shared_entity())
        .expect("test minecart should enter the loaded chunk");

    cart
}
