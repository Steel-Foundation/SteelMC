//! Independent loading and behavior delegation for the wool shearing example.
#![cfg(feature = "host")]

use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
    ptr,
    sync::Arc,
};
use steel_core::behavior::{BLOCK_BEHAVIORS, BlockBehaviorRegistry};
use steel_core::bootstrap::init_globals;
use steel_core::extension::ExtensionHooks;
use steel_extension_api::host;
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::locks::SyncMutex;

/// Build the example first, then run with `--ignored` to test real native loading.
#[test]
#[ignore = "requires cargo build --manifest-path extension-example/wool-shearing/Cargo.toml"]
fn loads_wool_shearing_and_preserves_original_behavior_delegation() -> Result<(), String> {
    let file = format!("{DLL_PREFIX}steel_wool_shearing_extension{DLL_SUFFIX}");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../extension-example/wool-shearing/target/debug")
        .join(file);
    let config = "";
    let captured = Arc::new(SyncMutex::new(None));
    let capture = Arc::clone(&captured);
    BlockBehaviorRegistry::on_registration(move |registry| {
        *capture.lock() = Some(registry.get_behavior_shared(&vanilla_blocks::WHITE_WOOL));
    })?;
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the example supplies the checked v2 entrypoints; load pins the library.
    let extension = unsafe { host::load(&path, config, &mut hooks) }?;
    assert_eq!(extension.name, "wool-shearing-example");
    assert!(!REGISTRY.is_initialized());
    assert!(BLOCK_BEHAVIORS.get().is_none());
    assert!(
        steel_extension_api::default_block_state("minecraft:stone".into())
            .match_owned(|_| false, |_| true)
    );
    init_globals();
    assert_eq!(
        host::initialize_all(vec![extension], &mut hooks)?,
        ["wool-shearing-example"]
    );
    let original = captured
        .lock()
        .take()
        .ok_or("block registration did not run")?;
    // SAFETY: reloading the same trusted module is rejected before pre-init repeats.
    assert!(unsafe { host::load(&path, config, &mut hooks) }.is_err());
    let replacement = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::WHITE_WOOL);
    assert!(!ptr::eq(original.as_ref(), replacement));
    assert_eq!(replacement.is_bed(), original.is_bed());
    assert_eq!(
        replacement.fits_inside_container_items(),
        original.fits_inside_container_items()
    );
    Ok(())
}
