//! Build script for steel-utils that generates translation constants.

use std::{fs, path::Path, process::Command};

use text_components::build::build_translations;

mod entity_events;
mod translations;

const FMT: bool = cfg!(feature = "fmt");

const OUT_DIR: &str = "src/generated";
const IDS: &str = "vanilla_translations/ids";
const REGISTRY: &str = "vanilla_translations/registry";
const ENTITY_EVENTS: &str = "entity_events";

/// Main build script entry point that generates translation constants.
pub fn main() {
    println!("cargo:rerun-if-changed=build/");

    if !Path::new(&format!("{OUT_DIR}/vanilla_translations")).exists() {
        fs::create_dir_all(format!("{OUT_DIR}/vanilla_translations"))
            .expect("Failed to create output directory");
    }

    let content = build_translations("build_assets/en_us.json");
    write_if_changed(format!("{OUT_DIR}/{IDS}.rs"), content.to_string());

    let content = translations::build();
    write_if_changed(format!("{OUT_DIR}/{REGISTRY}.rs"), content.to_string());

    let content = entity_events::build();
    write_if_changed(format!("{OUT_DIR}/{ENTITY_EVENTS}.rs"), content.to_string());

    if FMT && let Ok(entries) = fs::read_dir(OUT_DIR) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}

fn write_if_changed(path: impl AsRef<Path>, content: String) {
    let path = path.as_ref();
    if let Ok(existing) = fs::read_to_string(path)
        && existing == content
    {
        return;
    }

    if let Err(error) = fs::write(path, content) {
        panic!("Failed to write {}: {error}", path.display());
    }
}
