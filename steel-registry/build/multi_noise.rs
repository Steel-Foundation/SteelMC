use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

use crate::overworld_biome_builder::{BiomeEntry, OverworldBiomeBuilder};

/// Generate the Rust code for the multi-noise biome parameter list.
pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/overworld_biome_builder.json");

    // Build entries from the compact table data
    let builder = OverworldBiomeBuilder::from_json("build_assets/overworld_biome_builder.json");
    let entries = builder.build();

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated multi-noise biome source parameters.
        //!
        //! This file is auto-generated from build_assets/overworld_biome_builder.json.
        //! The biome entries are generated at build time using the OverworldBiomeBuilder logic.
        //! Do not edit manually.

        use crate::biome::BiomeRef;
        use crate::vanilla_biomes;
        use steel_utils::climate::{Parameter, ParameterList, ParameterPoint};
        use std::sync::LazyLock;
    });

    // Generate overworld biome parameters
    let overworld_entries = generate_biome_entries(&entries);

    stream.extend(quote! {
        /// Overworld biome parameter list for multi-noise biome selection.
        pub static OVERWORLD_BIOME_PARAMETERS: LazyLock<ParameterList<BiomeRef>> = LazyLock::new(|| {
            let entries = vec![
                #overworld_entries
            ];
            ParameterList::new(entries)
        });

        /// Get the biome for a target point in the overworld.
        #[inline]
        pub fn get_overworld_biome(target: &steel_utils::climate::TargetPoint) -> BiomeRef {
            *OVERWORLD_BIOME_PARAMETERS.find_value(target)
        }

        /// Get the biome with lastResult caching (matches vanilla's ThreadLocal warm-start).
        #[inline]
        pub fn get_overworld_biome_cached(target: &steel_utils::climate::TargetPoint, cache: &mut Option<usize>) -> BiomeRef {
            *OVERWORLD_BIOME_PARAMETERS.find_value_cached(target, cache)
        }
    });

    stream
}

/// Quantize a float value matching vanilla's `(long)(float * 10000.0F)`.
///
/// The JSON values are f64, but vanilla uses f32 arithmetic for quantization.
/// Casting to f32 first ensures bit-exact matching with Java's float precision.
fn quantize(v: f64) -> i64 {
    ((v as f32) * 10000.0f32) as i64
}

/// Convert a biome name like `"minecraft:plains"` to the vanilla_biomes constant
/// identifier `PLAINS`.
fn biome_ident(name: &str) -> Ident {
    let path = name.strip_prefix("minecraft:").unwrap_or(name);
    Ident::new(&path.to_uppercase(), Span::call_site())
}

fn generate_biome_entries(entries: &[BiomeEntry]) -> TokenStream {
    let entry_tokens: Vec<TokenStream> = entries
        .iter()
        .map(|entry| {
            // Quantize the float values to i64 matching vanilla's (long)(float * 10000.0F).
            let temp_min = quantize(entry.temperature.min);
            let temp_max = quantize(entry.temperature.max);
            let hum_min = quantize(entry.humidity.min);
            let hum_max = quantize(entry.humidity.max);
            let cont_min = quantize(entry.continentalness.min);
            let cont_max = quantize(entry.continentalness.max);
            let ero_min = quantize(entry.erosion.min);
            let ero_max = quantize(entry.erosion.max);
            let depth_min = quantize(entry.depth.min);
            let depth_max = quantize(entry.depth.max);
            let weird_min = quantize(entry.weirdness.min);
            let weird_max = quantize(entry.weirdness.max);
            let offset = quantize(entry.offset);

            let biome = biome_ident(&entry.biome);

            quote! {
                (
                    ParameterPoint::new(
                        Parameter::new(#temp_min, #temp_max),
                        Parameter::new(#hum_min, #hum_max),
                        Parameter::new(#cont_min, #cont_max),
                        Parameter::new(#ero_min, #ero_max),
                        Parameter::new(#depth_min, #depth_max),
                        Parameter::new(#weird_min, #weird_max),
                        #offset,
                    ),
                    &*vanilla_biomes::#biome,
                ),
            }
        })
        .collect();

    quote! {
        #(#entry_tokens)*
    }
}
