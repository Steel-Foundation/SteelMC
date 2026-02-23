//! Build script for steel-utils that generates translation constants.

use std::{fs, path::Path, process::Command};

use text_components::build::build_translations;

mod entity_events;
mod translations;

const FMT: bool = true;

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
    fs::write(format!("{OUT_DIR}/{IDS}.rs"), content.to_string())
        .expect("Failed to write translations ids file");

    let content = translations::build();
    fs::write(format!("{OUT_DIR}/{REGISTRY}.rs"), content.to_string())
        .expect("Failed to write translations registry file");

    let content = entity_events::build();
    fs::write(format!("{OUT_DIR}/{ENTITY_EVENTS}.rs"), content.to_string())
        .expect("Failed to write entity events file");

    if FMT && let Ok(entries) = fs::read_dir(OUT_DIR) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}
