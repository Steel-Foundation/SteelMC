//! Prints all blocks, items or entities with a non-default behavior implemented in `SteelMC`.
use std::process::ExitCode;

use steel_flint::adapter::{
    get_implemented_blocks, get_implemented_entities, get_implemented_items,
};

fn main() -> ExitCode {
    steel_flint::init();
    for id in get_implemented_blocks() {
        println!("{id}");
    }
    for id in get_implemented_items() {
        println!("{id}");
    }
    for id in get_implemented_entities() {
        println!("{id}");
    }
    ExitCode::SUCCESS
}
