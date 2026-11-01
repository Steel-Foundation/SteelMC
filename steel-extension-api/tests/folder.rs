//! Drop-in loading: an extensions folder with real libraries, junk and duplicates.
#![cfg(feature = "host")]
#![recursion_limit = "256"]
mod common;

use common::library;
use std::{env::consts::DLL_SUFFIX, fs};
use steel_core::extension::ExtensionHooks;
use steel_extension_api::host;

#[test]
#[ignore = "requires the extension-example and event-probe debug builds (see README)"]
fn plugins_folder_loads_libraries_and_skips_junk_duplicates_and_missing_dependencies()
-> Result<(), String> {
    let root = std::env::temp_dir().join(format!("steel-extension-folder-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let copy = |name: &str, file: &str| {
        fs::copy(library(name), root.join(format!("{file}{DLL_SUFFIX}")))
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    copy("vault", "a_vault")?;
    copy("essentials", "b_essentials")?;
    // WorldGuard without WorldEdit must be skipped, not fatal.
    copy("worldguard", "c_worldguard")?;
    // A second copy of Vault makes both copies ambiguous.
    copy("vault", "d_vault_again")?;
    fs::write(root.join(format!("e_junk{DLL_SUFFIX}")), b"not a library")
        .map_err(|e| e.to_string())?;
    fs::write(root.join("readme.txt"), b"ignored").map_err(|e| e.to_string())?;

    let files = host::discover(&root)?;
    assert_eq!(
        files.len(),
        5,
        "only library files are discovered: {files:?}"
    );
    let sources: Vec<_> = files
        .into_iter()
        .map(|path| host::ExtensionSource { path, config: None })
        .collect();
    let mut hooks = ExtensionHooks::default();
    // SAFETY: the libraries are this repository's extension projects built for the v3 contract.
    let report = unsafe { host::load_all(&sources, &root, &mut hooks) }?;
    let loaded: Vec<_> = report
        .extensions
        .iter()
        .map(|extension| extension.name)
        .collect();
    assert_eq!(loaded, ["essentials"]);
    assert_eq!(report.skipped.len(), 4, "{:#?}", report.skipped);
    let skipped = report.skipped.join("\n");
    assert!(skipped.contains("e_junk"), "{skipped}");
    assert!(
        skipped.contains("another file also declares extension vault"),
        "{skipped}"
    );
    assert!(
        skipped.contains("requires missing extension worldedit"),
        "{skipped}"
    );
    assert!(root.join("essentials/config.toml").is_file());
    assert_eq!(hooks.infos.len(), 1);
    assert_eq!(hooks.infos[0].api_version, steel_extension_api::API_VERSION);
    Ok(())
}
