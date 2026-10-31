//! Loads the independently compiled terrain example without extension configuration.
#![cfg(feature = "host")]

use rayon::ThreadPoolBuilder;
use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
    sync::Arc,
};
use steel_core::{
    bootstrap::init_globals, extension::ExtensionHooks, worldgen::WorldGeneratorRegistry,
};
use steel_extension_api::host;
use steel_registry::REGISTRY;
use toml::{Value, map::Map};

#[test]
#[ignore = "requires cargo build --manifest-path extension-example/strange-terrain/Cargo.toml"]
fn loads_terrain_without_game_of_life_configuration_and_resolves_its_generator()
-> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/strange-terrain/target/debug")
        .join(format!(
            "{DLL_PREFIX}steel_strange_terrain_extension{DLL_SUFFIX}"
        ));
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the separately built example supplies the checked v2 descriptor;
    // the loader pins the trusted library before retaining callbacks.
    let extension = unsafe { host::load(&path, "", &mut hooks) }?;
    assert!(!REGISTRY.is_initialized());
    init_globals();
    let registry = WorldGeneratorRegistry::new_with_builtins()?;
    host::initialize_all(vec![extension], &mut hooks)?;
    let key = "example:strange".parse().map_err(str::to_owned)?;
    let config = registry.validate_config(&key, &Value::Table(Map::new()))?;
    let pool = Arc::new(
        ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .map_err(|e| e.to_string())?,
    );
    registry.create(None, &config, 12345, pool)?;
    let unexpected = toml::from_str("unexpected = true").map_err(|e| e.to_string())?;
    assert!(registry.validate_config(&key, &unexpected).is_err());
    Ok(())
}
