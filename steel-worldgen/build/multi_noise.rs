use std::collections::BTreeMap;
use std::fs;

use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

/// A biome entry from the extracted multi-noise biome source parameter list.
#[derive(Deserialize)]
struct BiomeEntry {
    biome: String,
    parameters: BiomeParameters,
}

/// Climate parameters for a biome entry.
#[derive(Deserialize)]
struct BiomeParameters {
    temperature: [f64; 2],
    humidity: [f64; 2],
    continentalness: [f64; 2],
    erosion: [f64; 2],
    depth: [f64; 2],
    weirdness: [f64; 2],
    offset: f64,
}

/// Generate the Rust code for multi-noise biome parameter lists (all presets).
pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=../steel-registry/build_assets/multi_noise_biome_source_parameters.json"
    );

    let content = fs::read_to_string(
        "../steel-registry/build_assets/multi_noise_biome_source_parameters.json",
    )
    .expect("Failed to read multi_noise_biome_source_parameters.json");
    let presets: BTreeMap<String, Vec<BiomeEntry>> =
        serde_json::from_str(&content).expect("Failed to parse multi-noise biome parameters JSON");

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated multi-noise biome source parameters for all presets.
        //!
        //! Auto-generated from steel-registry/build_assets/multi_noise_biome_source_parameters.json.
        //! Do not edit manually.

        use steel_registry::biome::BiomeRef;
        use steel_registry::vanilla_biomes;
        use steel_utils::climate::{Parameter, ParameterList, ParameterPoint};
        use std::sync::LazyLock;
    });

    // Generate each preset
    for (preset_name, entries) in &presets {
        let short_name = preset_name
            .strip_prefix("minecraft:")
            .unwrap_or(preset_name);
        let upper_name = short_name.to_uppercase();

        let static_ident = Ident::new(&format!("{upper_name}_BIOME_PARAMETERS"), Span::call_site());
        let points_ident = Ident::new(&format!("{upper_name}_BIOME_POINTS"), Span::call_site());
        let lookup_fn = Ident::new(&format!("lookup_{short_name}_biome"), Span::call_site());
        let get_fn = Ident::new(&format!("get_{short_name}_biome"), Span::call_site());
        let get_cached_fn =
            Ident::new(&format!("get_{short_name}_biome_cached"), Span::call_site());

        let (points_tokens, arms_tokens) = generate_biome_entries(entries);
        let doc_static = format!(
            "{} biome parameter list for multi-noise biome selection.",
            capitalize(short_name)
        );
        let doc_get = format!("Get the biome for a target point in the {short_name}.");
        let doc_cached = format!(
            "Get the biome with lastResult caching for the {short_name} (matches vanilla's ThreadLocal warm-start)."
        );

        // Emit climate points as a `static` of `const`-constructed values so they live
        // in `.rodata` instead of being built inside the LazyLock closure. This keeps
        // LLVM from having to optimize a single multi-megabyte function full of
        // inlined `Parameter::new` / `ParameterPoint::new` calls.
        stream.extend(quote! {
            static #points_ident: &[ParameterPoint] = &[
                #points_tokens
            ];

            fn #lookup_fn(i: usize) -> BiomeRef {
                match i {
                    #arms_tokens
                    _ => unreachable!(),
                }
            }

            #[doc = #doc_static]
            pub static #static_ident: LazyLock<ParameterList<BiomeRef>> = LazyLock::new(|| {
                let entries: Vec<(ParameterPoint, BiomeRef)> = #points_ident
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, #lookup_fn(i)))
                    .collect();
                ParameterList::new(entries)
            });

            #[doc = #doc_get]
            #[inline]
            pub fn #get_fn(target: &steel_utils::climate::TargetPoint) -> BiomeRef {
                *#static_ident.find_value(target)
            }

            #[doc = #doc_cached]
            #[inline]
            pub fn #get_cached_fn(target: &steel_utils::climate::TargetPoint, cache: &mut Option<usize>) -> BiomeRef {
                *#static_ident.find_value_cached(target, cache)
            }
        });
    }

    stream
}

/// Quantize a float value matching vanilla's `(long)(float * 10000.0F)`.
///
/// The JSON values are f64, but vanilla uses f32 arithmetic for quantization.
/// Casting to f32 first ensures bit-exact matching with Java's float precision.
fn quantize(v: f64) -> i64 {
    ((v as f32) * 10000.0f32) as i64
}

/// Convert a biome name like `"minecraft:plains"` to the `vanilla_biomes` constant
/// identifier `PLAINS`.
fn biome_ident(name: &str) -> Ident {
    let path = name.strip_prefix("minecraft:").unwrap_or(name);
    Ident::new(&path.to_uppercase(), Span::call_site())
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

/// Build (points-static body, lookup-function match arms).
///
/// Splitting these lets the climate ranges live as compile-time `const`-evaluated
/// data while the biome reference resolution (which deref's `LazyLock<Biome>` and
/// can't be const) stays in a small runtime function.
fn generate_biome_entries(entries: &[BiomeEntry]) -> (TokenStream, TokenStream) {
    let mut points = Vec::with_capacity(entries.len());
    let mut arms = Vec::with_capacity(entries.len());

    for (i, entry) in entries.iter().enumerate() {
        let p = &entry.parameters;
        let temp_min = quantize(p.temperature[0]);
        let temp_max = quantize(p.temperature[1]);
        let hum_min = quantize(p.humidity[0]);
        let hum_max = quantize(p.humidity[1]);
        let cont_min = quantize(p.continentalness[0]);
        let cont_max = quantize(p.continentalness[1]);
        let ero_min = quantize(p.erosion[0]);
        let ero_max = quantize(p.erosion[1]);
        let depth_min = quantize(p.depth[0]);
        let depth_max = quantize(p.depth[1]);
        let weird_min = quantize(p.weirdness[0]);
        let weird_max = quantize(p.weirdness[1]);
        let offset = quantize(p.offset);

        let biome = biome_ident(&entry.biome);

        points.push(quote! {
            ParameterPoint::new(
                Parameter::new(#temp_min, #temp_max),
                Parameter::new(#hum_min, #hum_max),
                Parameter::new(#cont_min, #cont_max),
                Parameter::new(#ero_min, #ero_max),
                Parameter::new(#depth_min, #depth_max),
                Parameter::new(#weird_min, #weird_max),
                #offset,
            ),
        });

        arms.push(quote! {
            #i => &*vanilla_biomes::#biome,
        });
    }

    let points_tokens = quote! { #(#points)* };
    let arms_tokens = quote! { #(#arms)* };
    (points_tokens, arms_tokens)
}
