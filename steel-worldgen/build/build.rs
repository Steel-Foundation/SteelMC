#![allow(
    dead_code,
    missing_docs,
    reason = "build-script modules include parser shapes and generated-code helpers"
)]

use std::{env, fs, path::Path};

use steel_utils as _;

#[path = "../../build/codegen.rs"]
mod codegen;
use codegen::write_tokens_if_changed;

mod density;
mod multi_noise;
mod noise_parameters;

const MULTI_NOISE: &str = "multi_noise";
const NOISE_PARAMETERS: &str = "noise_parameters";

pub fn main() {
    println!("cargo:rerun-if-changed=build/");
    println!("cargo:rerun-if-changed=../build/codegen.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let out_dir = Path::new(&manifest_dir).join("src/generated");
    fs::create_dir_all(&out_dir).expect("failed to create worldgen generated directory");

    let generated_files = [
        (multi_noise::build(), MULTI_NOISE),
        (noise_parameters::build(), NOISE_PARAMETERS),
    ];

    for (content, file_name) in generated_files {
        let path = out_dir.join(format!("vanilla_{file_name}.rs"));
        write_tokens_if_changed(&path, content);
    }

    let df = density::build();
    let df_dir = out_dir.join("vanilla_density_functions");
    fs::create_dir_all(&df_dir).expect("failed to create generated density function directory");

    for (content, name) in [
        (df.overworld, "overworld"),
        (df.nether, "nether"),
        (df.end, "end"),
    ] {
        let path = df_dir.join(format!("{name}.rs"));
        write_tokens_if_changed(&path, content);
    }

    let path = df_dir.join("mod.rs");
    write_tokens_if_changed(&path, df.index);
}
