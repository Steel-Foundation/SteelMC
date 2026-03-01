use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// JSON structure for a noise parameter entry (matches datapack format).
#[derive(Deserialize, Debug)]
struct NoiseParamsJson {
    #[serde(rename = "firstOctave")]
    first_octave: i32,
    amplitudes: Vec<f64>,
}

/// Generate noise parameters code from the vanilla datapack.
pub(crate) fn build() -> TokenStream {
    let noise_dir =
        Path::new("build_assets/builtin_datapacks/minecraft/data/minecraft/worldgen/noise");

    println!("cargo:rerun-if-changed={}", noise_dir.display());

    let mut noises: BTreeMap<String, NoiseParamsJson> = BTreeMap::new();

    for entry in fs::read_dir(noise_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to read noise directory {}: {e}",
            noise_dir.display()
        )
    }) {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "json") {
            let name = path
                .file_stem()
                .expect("No file stem")
                .to_str()
                .expect("Non-UTF8 filename")
                .to_string();

            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));

            let params: NoiseParamsJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", path.display()));

            noises.insert(name, params);
        }
    }

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated vanilla noise parameters from the datapack.
        //!
        //! Auto-generated from `builtin_datapacks/minecraft/data/minecraft/worldgen/noise/*.json`.
        //! Do not edit manually.

        use rustc_hash::FxHashMap;
        use steel_utils::density::NoiseParameters;
    });

    // Generate static amplitude arrays
    for (name, params) in &noises {
        let const_name = Ident::new(
            &format!("{}_AMPLITUDES", name.to_uppercase()),
            Span::call_site(),
        );
        let amplitudes = &params.amplitudes;

        stream.extend(quote! {
            static #const_name: &[f64] = &[#(#amplitudes),*];
        });
    }

    // Generate the get_noise_parameters function
    let entries: Vec<TokenStream> = noises
        .iter()
        .map(|(name, params)| {
            let amp_name = Ident::new(
                &format!("{}_AMPLITUDES", name.to_uppercase()),
                Span::call_site(),
            );
            let first_octave = params.first_octave;
            let key = format!("minecraft:{name}");

            quote! {
                (String::from(#key), NoiseParameters::new(#first_octave, #amp_name.to_vec())),
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
