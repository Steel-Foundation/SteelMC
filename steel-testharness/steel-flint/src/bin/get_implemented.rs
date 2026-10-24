//! Prints all blocks, items or entities with a non-default behavior implemented in `SteelMC`.
use std::env;
use std::process::ExitCode;

use steel_flint::adapter::{
    get_implemented_blocks, get_implemented_entities, get_implemented_items,
};

fn main() -> ExitCode {
    let kind = env::args().nth(1);
    let get = match kind.as_deref() {
        Some("blocks") => get_implemented_blocks,
        Some("items") => get_implemented_items,
        Some("entities") => get_implemented_entities,
        _ => {
            eprintln!("usage: steel-implemented <blocks|items|entities>");
            return ExitCode::FAILURE;
        }
    };
    steel_flint::init();
    for id in get() {
        println!("{id}");
    }
    ExitCode::SUCCESS
}
