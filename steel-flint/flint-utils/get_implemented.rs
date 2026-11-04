//! Lists all blocks, items or entities with a non-default behavior implemented in `SteelMC`.
//!
//! Prints one sorted, deduplicated id per line to stdout, or writes them to the file given as the
//! first argument.
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::process::ExitCode;

use steel_flint::adapter::{
    get_implemented_blocks, get_implemented_entities, get_implemented_items,
};

fn main() -> ExitCode {
    steel_flint::init();
    let ids: BTreeSet<String> = get_implemented_blocks()
        .into_iter()
        .chain(get_implemented_items())
        .chain(get_implemented_entities())
        .collect();

    let Some(path) = env::args().nth(1) else {
        for id in &ids {
            println!("{id}");
        }
        return ExitCode::SUCCESS;
    };

    let lines = ids.iter().fold(String::new(), |acc, id| acc + id + "\n");
    if let Err(err) = fs::write(&path, lines) {
        eprintln!("failed to write {path}: {err}");
        return ExitCode::FAILURE;
    }
    println!("{} implemented ids", ids.len());
    ExitCode::SUCCESS
}
