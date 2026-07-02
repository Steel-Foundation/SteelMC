#![expect(missing_docs, reason = "internal build script")]
#![expect(
    clippy::disallowed_types,
    reason = "build script lacks project type aliases"
)]

use heck::ToShoutySnakeCase;
use proc_macro2::Span;
use serde::Deserialize;
use std::{env, fs, path::Path};
use syn::Ident;

mod blocks;
mod candle_cakes;
mod common;
mod entities;
mod items;
mod strippables;
mod waxables;
mod weathering;

#[derive(Debug, Deserialize)]
struct Classes {
    blocks: Vec<blocks::BlockClass>,
    items: Vec<items::ItemClass>,
    #[serde(default)]
    entities: Vec<entities::EntityClass>,
}

pub fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let behavior_out_dir = format!("{manifest_dir}/src/behavior/generated");
    let entity_out_dir = format!("{manifest_dir}/src/entity/generated");

    let classes_json = fs::read_to_string(format!("{manifest_dir}/build/classes.json"))
        .expect("Failed to read classes.json");
    let classes: Classes =
        serde_json::from_str(&classes_json).expect("Failed to parse classes.json");

    fs::create_dir_all(&behavior_out_dir).expect("Failed to create behavior output directory");
    fs::create_dir_all(&entity_out_dir).expect("Failed to create entity output directory");

    write_if_changed(
        format!("{behavior_out_dir}/blocks.rs"),
        blocks::build(&classes.blocks),
    );
    write_if_changed(
        format!("{behavior_out_dir}/candle_cakes.rs"),
        candle_cakes::build(),
    );
    write_if_changed(
        format!("{behavior_out_dir}/items.rs"),
        items::build(&classes.items),
    );
    write_if_changed(format!("{behavior_out_dir}/waxables.rs"), waxables::build());
    write_if_changed(
        format!("{behavior_out_dir}/weathering.rs"),
        weathering::build(),
    );
    write_if_changed(
        format!("{behavior_out_dir}/strippables.rs"),
        strippables::build(),
    );
    write_if_changed(
        format!("{entity_out_dir}/entities.rs"),
        entities::build(&classes.entities),
    );

    println!("cargo:rerun-if-changed={manifest_dir}/build/classes.json");
    println!("cargo:rerun-if-changed={manifest_dir}/src/behavior/blocks");
    println!("cargo:rerun-if-changed={manifest_dir}/src/behavior/items");
    println!("cargo:rerun-if-changed={manifest_dir}/src/entity/entities");
}

/// Items use lowercase field names (`vanilla_items::ITEMS.stone`)
#[must_use]
fn to_item_ident(name: &str) -> Ident {
    Ident::new(name, Span::call_site())
}

/// Blocks use `SCREAMING_SNAKE_CASE` constants (`vanilla_blocks::STONE`)
#[must_use]
pub fn to_block_ident(name: &str) -> Ident {
    Ident::new(&name.to_shouty_snake_case(), Span::call_site())
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
