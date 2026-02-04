//! Build script for steel-utils that generates translation constants.

use std::{fs, path::Path, process::Command};

use text_components::build::build_translations;

mod translations;

const FMT: bool = true;

const OUT_DIR: &str = "src/generated/vanilla_translations";
const IDS: &str = "ids";
const REGISTRY: &str = "registry";

/// Main build script entry point that generates translation constants.
pub fn main() {
    println!("cargo:rerun-if-changed=build/");

    if !Path::new(OUT_DIR).exists() {
        fs::create_dir_all(OUT_DIR).expect("Failed to create output directory");
    }

    let content = build_translations("build_assets/en_us.json");
    fs::write(format!("{OUT_DIR}/{IDS}.rs"), content.to_string())
        .expect("Failed to write translations ids file");

    let content = translations::build();
    fs::write(format!("{OUT_DIR}/{REGISTRY}.rs"), content.to_string())
        .expect("Failed to write translations registry file");

    if FMT && let Ok(entries) = fs::read_dir(OUT_DIR) {
        for entry in entries.flatten() {
            let _ = Command::new("rustfmt").arg(entry.path()).output();
        }
    }
}
