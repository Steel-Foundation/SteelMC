#![expect(
    missing_docs,
    reason = "integration test module does not require public crate docs"
)]

use std::fs;
use steel_loader::{DependencyResolver, ModDiscovery, ModLoader};

#[test]
fn test_mod_loader_integration() {
    let temp_dir = std::env::temp_dir().join("steel_loader_test_mods");
    if temp_dir.exists() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    fs::create_dir_all(&temp_dir).unwrap();

    let mod_dir = temp_dir.join("test_mod");
    fs::create_dir_all(&mod_dir).unwrap();

    let manifest_content = r#"{
        "schemaVersion": 1,
        "id": "test_mod",
        "version": "1.0.0",
        "name": "Test Integration Mod",
        "entrypoints": {
            "main": []
        }
    }"#;

    fs::write(mod_dir.join("steel.mod.json"), manifest_content).unwrap();

    let discovered = ModDiscovery::discover_all(&temp_dir);
    assert_eq!(discovered.len(), 1);
    assert_eq!(discovered[0].manifest.id, "test_mod");

    let resolved = DependencyResolver::resolve(discovered).unwrap();
    assert_eq!(resolved.len(), 1);

    let mut loader = ModLoader::new();
    let dummy_server_ptr = std::ptr::null_mut();

    // SAFETY: Loading verified discovered mod with dummy server pointer.
    unsafe {
        loader
            .load_and_init(resolved.into_iter().next().unwrap(), dummy_server_ptr)
            .unwrap();
    }

    assert_eq!(loader.loaded_mods().len(), 1);

    // SAFETY: Shutting down loaded mods.
    unsafe {
        loader.shutdown(dummy_server_ptr);
    }

    assert_eq!(loader.loaded_mods().len(), 0);

    let _ = fs::remove_dir_all(&temp_dir);
}
