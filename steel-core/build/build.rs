#![allow(missing_docs)]

use std::fs;

use serde::Deserialize;

mod blocks;
mod items;
mod weathering;

const OUT_DIR: &str = "src/behavior/generated";

#[derive(Debug, Deserialize)]
struct Classes {
    blocks: Vec<blocks::BlockClass>,
    items: Vec<items::ItemClass>,
}

pub fn main() {
    let classes_json =
        fs::read_to_string("build/classes.json").expect("Failed to read classes.json");
    let classes: Classes =
        serde_json::from_str(&classes_json).expect("Failed to parse classes.json");

    fs::create_dir_all(OUT_DIR).expect("Failed to create output directory");

    fs::write(
        format!("{OUT_DIR}/blocks.rs"),
        blocks::build(&classes.blocks),
    )
    .expect("Failed to write blocks.rs");
    fs::write(format!("{OUT_DIR}/items.rs"), items::build(&classes.items))
        .expect("Failed to write items.rs");
    fs::write(format!("{OUT_DIR}/weathering.rs"), weathering::build())
        .expect("Failed to write weathering.rs");

    println!("cargo:rerun-if-changed=build/classes.json");
}
