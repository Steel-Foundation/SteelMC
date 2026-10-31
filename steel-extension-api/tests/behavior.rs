//! A real independently compiled native extension chooses its own delegation policy.
#![cfg(feature = "host")]
use std::ptr;
use std::{
    env::consts::{DLL_PREFIX, DLL_SUFFIX},
    path::PathBuf,
    sync::{Arc, Weak},
};
use steel_core::{
    behavior::{BLOCK_BEHAVIORS, BlockBehaviorRegistry, RailBehavior},
    bootstrap::init_globals,
    extension::ExtensionHooks,
    worldgen::WorldGeneratorRegistry,
};
use steel_extension_api::host;
use steel_registry::{REGISTRY, item_stack::ItemStack, vanilla_blocks};
use steel_utils::BlockPos;
use steel_utils::locks::SyncMutex;

#[test]
#[ignore = "requires cargo build --manifest-path steel-extension-api/tests/fixture/Cargo.toml"]
fn independently_compiled_extension_inherits_removes_and_replaces_native_capabilities()
-> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixture/target/debug")
        .join(format!(
            "{DLL_PREFIX}steel_behavior_boundary_fixture{DLL_SUFFIX}"
        ));
    let captured = Arc::new(SyncMutex::new(None));
    let capture = Arc::clone(&captured);
    BlockBehaviorRegistry::on_registration(move |registry| {
        *capture.lock() = Some((
            registry.get_behavior_shared(&vanilla_blocks::CARROTS),
            registry.get_behavior_shared(&vanilla_blocks::POTATOES),
            registry.get_behavior_shared(&vanilla_blocks::RAIL),
            registry.get_behavior_shared(&vanilla_blocks::COMPARATOR),
        ));
    })?;
    let mut hooks = ExtensionHooks::default();
    // SAFETY: this separately compiled fixture honors the checked v2 descriptor;
    // load pins it before invoking pre-init or retaining any callback.
    let extension = unsafe { host::load(&path, "", &mut hooks) }?;
    assert!(!REGISTRY.is_initialized());
    assert!(BLOCK_BEHAVIORS.get().is_none());
    init_globals();
    let _generators = WorldGeneratorRegistry::new_with_builtins()?;
    assert_eq!(
        host::initialize_all(vec![extension], &mut hooks)?,
        ["behavior-boundary-fixture"]
    );
    let (original, potatoes, original_rail, original_comparator) = captured
        .lock()
        .take()
        .ok_or("block registration did not run")?;

    let replaced = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::CARROTS);
    assert!(!ptr::eq(original.as_ref(), replaced));
    assert!(original.as_bonemealable().is_some());
    assert!(replaced.as_bonemealable().is_some());
    assert!(potatoes.as_bonemealable().is_some());
    assert!(
        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::POTATOES)
            .as_bonemealable()
            .is_none()
    );

    let custom_rail = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::RAIL);
    assert_eq!(
        original_rail.as_rail().map(RailBehavior::is_straight),
        Some(false)
    );
    assert_eq!(
        custom_rail.as_rail().map(RailBehavior::is_straight),
        Some(true)
    );
    let picked = replaced.get_clone_item_stack(
        &vanilla_blocks::CARROTS,
        vanilla_blocks::CARROTS.default_state(),
        None,
        false,
    );
    let native_picked = original.get_clone_item_stack(
        &vanilla_blocks::CARROTS,
        vanilla_blocks::CARROTS.default_state(),
        None,
        false,
    );
    assert_eq!(
        picked.as_ref().map(ItemStack::item),
        native_picked.as_ref().map(ItemStack::item)
    );

    let comparator = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::COMPARATOR);
    let created = comparator.new_block_entity(
        Weak::new(),
        BlockPos::new(0, 64, 0),
        vanilla_blocks::COMPARATOR.default_state(),
    );
    let entity = created
        .into_created()
        .ok_or("fixture did not produce its wrapped comparator entity")?;
    assert!(entity.trigger_event(1234, 17));
    let saved = entity.save_without_metadata();
    assert_eq!(saved.int("extension_count"), Some(17));
    assert_eq!(saved.int("OutputSignal"), Some(0));
    entity
        .load_with_owned_components(&saved)
        .map_err(|e| e.to_string())?;
    assert_eq!(
        entity.save_without_metadata().int("extension_count"),
        Some(17)
    );
    drop(entity);
    assert!(
        original_comparator
            .new_block_entity(
                Weak::new(),
                BlockPos::new(0, 64, 0),
                vanilla_blocks::COMPARATOR.default_state()
            )
            .into_created()
            .is_some()
    );
    Ok(())
}
