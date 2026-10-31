//! Registration failure stops post-init; the normal one-time engine initialization still freezes registries.
#![cfg(feature = "host")]
use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
};
use steel_core::{
    behavior::BLOCK_BEHAVIORS, bootstrap::init_globals, extension::ExtensionHooks,
    worldgen::WorldGeneratorRegistry,
};
use steel_extension_api::host;
use steel_registry::REGISTRY;
#[test]
#[ignore = "requires cargo build --manifest-path steel-extension-api/tests/fixture/Cargo.toml"]
fn registration_failure_prevents_the_init_entrypoint() -> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixture/target/debug")
        .join(format!(
            "{DLL_PREFIX}steel_behavior_boundary_fixture{DLL_SUFFIX}"
        ));
    let mut hooks = ExtensionHooks::default();
    // SAFETY: this separately built fixture supplies the checked v2 descriptor;
    // load pins the module before invoking any entrypoint or retaining callbacks.
    let extension = unsafe { host::load(&path, "fail_after_replacement", &mut hooks) }?;
    assert!(!REGISTRY.is_initialized());
    init_globals();
    let _generators = WorldGeneratorRegistry::new_with_builtins()?;
    assert!(BLOCK_BEHAVIORS.get().is_some());
    let error = host::initialize_all(vec![extension], &mut hooks)
        .expect_err("failed registration must stop init");
    assert!(error.contains("fixture failed after replacing carrots"));
    assert!(!error.contains("init ran after failed registration"));
    Ok(())
}
