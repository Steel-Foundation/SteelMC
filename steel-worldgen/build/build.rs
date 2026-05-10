#![allow(
    dead_code,
    missing_docs,
    reason = "build-script modules include parser shapes and generated-code helpers"
)]

use std::{env, fs, path::Path, process::Command};

mod density;
mod density_functions;
mod multi_noise;
mod noise_parameters;
mod surface_rules;

const FMT: bool = cfg!(feature = "fmt");

const MULTI_NOISE: &str = "multi_noise";
const NOISE_PARAMETERS: &str = "noise_parameters";

pub fn main() {
    println!("cargo:rerun-if-changed=build/");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = Path::new(&manifest_dir).join("src/generated");
    fs::create_dir_all(&out_dir).expect("failed to create worldgen generated directory");

    let generated_files = [
        (multi_noise::build(), MULTI_NOISE),
        (noise_parameters::build(), NOISE_PARAMETERS),
    ];

    for (content, file_name) in generated_files {
        let path = out_dir.join(format!("vanilla_{file_name}.rs"));
        let content = content.to_string();
        if fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
            continue;
        }
        fs::write(&path, content).expect("failed to write generated worldgen file");
    }

    let df = density_functions::build();
    let df_dir = out_dir.join("vanilla_density_functions");
    fs::create_dir_all(&df_dir).expect("failed to create generated density function directory");

    for (content, name) in [
        (df.overworld, "overworld"),
        (df.nether, "nether"),
        (df.end, "end"),
    ] {
        let path = df_dir.join(format!("{name}.rs"));
        let content = content.to_string();
        if fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
            continue;
        }
        fs::write(&path, content).expect("failed to write generated density function file");
    }

    let path = df_dir.join("mod.rs");
    let content = df.index.to_string();
    if !fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        fs::write(&path, content).expect("failed to write generated density function index");
    }

    if FMT && let Ok(entries) = fs::read_dir(&df_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let _ = Command::new("rustfmt").arg(path).output();
        }
    }
}
