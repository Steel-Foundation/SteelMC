#![expect(missing_docs, reason = "internal build script")]
#![expect(
    clippy::disallowed_types,
    reason = "build script lacks project type aliases"
)]

use serde::Deserialize;
use std::env;
use std::fs;

mod blocks;
mod common;
mod items;
mod waxables;
mod weathering;

#[derive(Debug, Deserialize)]
struct Classes {
    blocks: Vec<blocks::BlockClass>,
    items: Vec<items::ItemClass>,
}

pub fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = format!("{manifest_dir}/src/behavior/generated");

    let classes_json = fs::read_to_string(format!("{manifest_dir}/build/classes.json"))
        .expect("Failed to read classes.json");
    let classes: Classes =
        serde_json::from_str(&classes_json).expect("Failed to parse classes.json");

    fs::create_dir_all(&out_dir).expect("Failed to create output directory");

    fs::write(
        format!("{out_dir}/blocks.rs"),
        blocks::build(&classes.blocks),
    )
    .expect("Failed to write blocks.rs");
    fs::write(format!("{out_dir}/items.rs"), items::build(&classes.items))
        .expect("Failed to write items.rs");
    fs::write(format!("{out_dir}/waxables.rs"), waxables::build())
        .expect("Failed to write waxables.rs");
    fs::write(format!("{out_dir}/weathering.rs"), weathering::build())
        .expect("Failed to write weathering.rs");

    println!("cargo:rerun-if-changed={manifest_dir}/build/classes.json");
    println!("cargo:rerun-if-changed={manifest_dir}/src/behavior/blocks");
    println!("cargo:rerun-if-changed={manifest_dir}/src/behavior/items");
}
