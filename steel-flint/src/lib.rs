//! Flint testing framework integration for `SteelMC`.

#![recursion_limit = "256"]

pub mod adapter;
mod convert;
mod player;
mod test_connection;
mod world;

pub use adapter::SteelAdapter;
pub use player::SteelTestPlayer;
pub use world::SteelTestWorld;

pub use flint_core::{TestLoader, TestRunner};

use std::sync::{Arc, LazyLock};
use steel_core::behavior::init_behaviors;
use steel_core::block_entity::init_block_entities;
use steel_core::entity::init_entities;
use steel_registry::init_vanilla_registry;
use tokio::runtime::{Builder, Runtime};

/// Shared Tokio runtime for flint tests.
static RUNTIME: LazyLock<Arc<Runtime>> = LazyLock::new(|| {
    Arc::new(
        Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("Failed to create Flint runtime"),
    )
});

/// Initialize the `SteelMC` registry and behaviors for testing.
///
/// Must be called before creating any test worlds or adapters.
/// Safe to call multiple times.
pub fn init() {
    init_vanilla_registry();
    init_behaviors();
    init_block_entities();
    init_entities();
}

pub(crate) fn runtime() -> Arc<Runtime> {
    RUNTIME.clone()
}
