//! The world-tick example initializes independently with its own board configuration.
#![cfg(feature = "host")]

use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
};
use steel_core::{bootstrap::init_globals, extension::ExtensionHooks};
use steel_extension_api::host;
use steel_registry::REGISTRY;

#[test]
#[ignore = "requires cargo build --manifest-path extension-example/game-of-life/Cargo.toml"]
fn loads_game_of_life_with_only_its_board_configuration() -> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/game-of-life/target/debug")
        .join(format!(
            "{DLL_PREFIX}steel_game_of_life_extension{DLL_SUFFIX}"
        ));
    let config = "[life]\nworld = 'minecraft:overworld'\nx = -1\ny = 96\nz = -1\nwidth = 5\ndepth = 5\ninterval = 10\n";
    let mut hooks = ExtensionHooks::default();
    // SAFETY: this separately built trusted example supplies the checked v2
    // descriptor; load pins it for the lifetime of its world-tick callback.
    let extension = unsafe { host::load(&path, config, &mut hooks) }?;
    assert!(!REGISTRY.is_initialized());
    init_globals();
    host::initialize_all(vec![extension], &extension_data_root(), &mut hooks)?;
    Ok(())
}

fn extension_data_root() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("steel-extension-api-tests-{}", std::process::id()))
}
