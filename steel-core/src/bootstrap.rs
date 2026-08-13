//! Global registry and behavior initialization.

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

/// # Errors
/// Returns an error if the global registry has already been initialized.
pub(crate) fn init_globals() -> Result<(), String> {
    let start = Instant::now();
    let published = init_vanilla_registry();
    log::info!("Vanilla registry loaded in {:?}", start.elapsed());

    if !published {
        return Err("global registry has already been initialized".to_owned());
    }

    fill_behavior_registries();
    Ok(())
}

/// Idempotent [`init_globals`] for in-process test and benchmark runtimes.
#[cfg(any(test, feature = "benchmark-support", feature = "test-harness"))]
pub fn init_globals_once() {
    use std::sync::Once;

    static INIT: Once = Once::new();
    INIT.call_once(|| {
        init_vanilla_registry();
        fill_behavior_registries();
    });
}
