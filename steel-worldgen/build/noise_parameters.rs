use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// JSON structure for a noise parameter entry (matches datapack format).
///
/// Mirrors vanilla's `NormalNoise.Parameters` codec. `normalize` is modeled as a plain
/// bool (`enabled`/`disabled`, matching vanilla's `Codec.BOOL` branch) — the deprecated
/// `"legacy"` string form never appears in extracted data.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct NoiseParamsJson {
    #[serde(default = "default_base_amplitude")]
    base_amplitude: f64,
    base_octave: i32,
    #[serde(default = "default_octave_count")]
    octave_count: i32,
    #[serde(default = "default_normalize")]
    normalize: bool,
    #[serde(default)]
    amplitude_modifiers: Vec<f64>,
}

const fn default_base_amplitude() -> f64 {
    1.0
}

const fn default_octave_count() -> i32 {
    1
}

const fn default_normalize() -> bool {
    true
}

/// Recursively collect all `.json` files under `dir`, keyed by their path
/// relative to `base` (without extension). E.g. `nether/temperature`.
fn collect_noise_files(base: &Path, dir: &Path, out: &mut BTreeMap<String, NoiseParamsJson>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("Failed to read noise directory {}: {e}", dir.display()))
    {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.is_dir() {
            collect_noise_files(base, &path, out);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let relative = path
                .strip_prefix(base)
                .expect("path not under base")
                .with_extension("");
            let name = relative.to_str().expect("Non-UTF8 path").replace('\\', "/");

            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));

            let params: NoiseParamsJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

            if !params.amplitude_modifiers.is_empty()
                && params.amplitude_modifiers.len() as i32 != params.octave_count
            {
                panic!(
                    "{}: amplitude_modifiers had size {}, but octave_count was {}",
                    path.display(),
                    params.amplitude_modifiers.len(),
                    params.octave_count
                );
            }

            out.insert(name, params);
        }
    }
}

/// Generate noise parameters code from the vanilla datapack.
pub(crate) fn build() -> TokenStream {
    let noise_dir =
        Path::new("../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/noise");

    println!("cargo:rerun-if-changed={}", noise_dir.display());

    let mut noises: BTreeMap<String, NoiseParamsJson> = BTreeMap::new();
    collect_noise_files(noise_dir, noise_dir, &mut noises);

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated vanilla noise parameters from the datapack.
        //!
        //! Auto-generated from `steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/noise/*.json`.
        //! Do not edit manually.

        use rustc_hash::FxHashMap;
        use steel_worldgen::density::NoiseParameters;
    });

    // Generate static amplitude-modifier arrays
    for (name, params) in &noises {
        let const_name = Ident::new(
            &format!(
                "{}_AMPLITUDE_MODIFIERS",
                name.replace('/', "_").to_uppercase()
            ),
            Span::call_site(),
        );
        let amplitude_modifiers = &params.amplitude_modifiers;

        stream.extend(quote! {
            static #const_name: &[f64] = &[#(#amplitude_modifiers),*];
        });
    }

    // Generate the get_noise_parameters function
    let entries: Vec<TokenStream> = noises
        .iter()
        .map(|(name, params)| {
            let amp_name = Ident::new(
                &format!("{}_AMPLITUDE_MODIFIERS", name.replace('/', "_").to_uppercase()),
                Span::call_site(),
            );
            let base_amplitude = params.base_amplitude;
            let base_octave = params.base_octave;
            let octave_count = params.octave_count;
            let normalize = params.normalize;
            let key = format!("minecraft:{name}");

            quote! {
                (String::from(#key), NoiseParameters::new(#base_amplitude, #base_octave, #octave_count, #normalize, #amp_name.to_vec())),
            }
        })
        .collect();

    stream.extend(quote! {
        /// Get all vanilla noise parameters from the datapack.
        ///
        /// Returns a map keyed by namespaced noise ID (e.g., `"minecraft:temperature"`).
        #[must_use]
        pub fn get_noise_parameters() -> FxHashMap<String, NoiseParameters> {
            FxHashMap::from_iter([
                #(#entries)*
            ])
        }
    });

    stream
}
