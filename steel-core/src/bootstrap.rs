//! Global registry and behavior initialization.

use std::sync::Once;
use std::time::Instant;

use steel_registry::init_vanilla_registry;

use crate::behavior::init_behaviors;
use crate::block_entity::init_block_entities;
use crate::entity::init_entities;

fn fill_behavior_registries() {
    init_behaviors();
    init_block_entities();
    init_entities();
    log::info!("Behavior registries initialized");
}

/// Initializes the vanilla registry and the behavior registries.
///
/// Idempotent, so an embedder loading several worlds in one process bootstraps once.
pub fn init_globals() {
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        let start = Instant::now();
        init_vanilla_registry();
        log::info!("Vanilla registry loaded in {:?}", start.elapsed());
        fill_behavior_registries();
    });
}
