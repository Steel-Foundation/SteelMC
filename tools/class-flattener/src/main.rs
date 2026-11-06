//! Flattens a decompiled vanilla class: its whole superclass chain and
//! interface defaults in one listing, with every `super` call resolved to the
//! implementation it reaches and every inherited `this` call that this class
//! overrides marked.
//!
//! Usage: `cargo run --release --manifest-path tools/class-flattener/Cargo.toml -- Chicken`

mod calls;
mod hierarchy;
mod model;
mod render;
mod source;
#[cfg(test)]
mod tests;

use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::Parser;

use crate::hierarchy::Hierarchy;
use crate::render::{Options, Renderer};
use crate::source::SourceTree;

#[derive(Parser)]
#[command(about = "Flatten a vanilla Java class from minecraft-src")]
struct Cli {
    /// Class to flatten: simple (`Chicken`), nested (`Fox.FoxBreedGoal`) or
    /// fully qualified name.
    class: String,

    /// Decompiled source root.
    #[arg(long, default_value = concat!(env!("CARGO_MANIFEST_DIR"), "/../../minecraft-src/minecraft/src"))]
    src: PathBuf,

    /// Only show these methods (repeatable); `<init>` selects constructors.
    #[arg(short, long = "method", value_name = "NAME")]
    methods: Vec<String>,

    /// Inline the bodies that super calls reach instead of listing them
    /// separately.
    #[arg(short, long)]
    inline: bool,

    /// Only print the member index without bodies.
    #[arg(short, long)]
    summary: bool,

    /// Print tab-separated declaration and dispatch rows for aggregate
    /// analysis instead of the listing.
    #[arg(long, conflicts_with_all = ["inline", "summary", "methods"])]
    tsv: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(output) => {
            // A closed pipe (`| head`) is not an error for a listing tool.
            match io::stdout().lock().write_all(output.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) if err.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("error: failed to write output: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<String, String> {
    let source = SourceTree::new(cli.src.clone())?;
    let target = source.find_class(&cli.class)?;
    let hierarchy = Hierarchy::build(&source, target);
    let options = Options {
        methods: cli.methods.clone(),
        inline: cli.inline,
        summary: cli.summary,
        source_label: source_label(&cli.src),
    };
    let renderer = Renderer::new(&source, &hierarchy, &options);
    Ok(if cli.tsv {
        renderer.records()
    } else {
        renderer.render()
    })
}

/// `minecraft-src (26.3)` from the `GitCraft` commit subject, when available.
fn source_label(src: &Path) -> String {
    let version = Command::new("git")
        .arg("-C")
        .arg(src)
        .args(["log", "-1", "--format=%s"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .filter(|v| !v.is_empty());
    match version {
        Some(version) => format!("minecraft-src ({version})"),
        None => format!("{} (version unknown)", src.display()),
    }
}
