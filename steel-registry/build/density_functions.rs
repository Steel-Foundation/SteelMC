use proc_macro2::TokenStream;
use quote::quote;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::{fs, path::PathBuf};

/// Parsed density function from datapack JSON.
///
/// Values in the datapack format are polymorphic:
/// - Bare number -> `Constant`
/// - Bare string -> `Reference`
/// - Object with `"type"` field -> `Data` (tag-based dispatch via `DensityFunctionData`)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum DensityFunctionJson {
    Constant(f64),
    Reference(String),
    Data(DensityFunctionData),
}

/// Internally-tagged serde representation of typed density function objects.
///
/// Uses `#[serde(tag = "type")]` to dispatch on the `"type"` field, with
/// `#[serde(rename)]` on each variant to match the `minecraft:` prefixed names.
/// Field names are mapped with `#[serde(rename)]` where the JSON key differs
/// from the Rust field name.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum DensityFunctionData {
    #[serde(rename = "minecraft:constant")]
    Constant {
        #[serde(alias = "argument")]
        value: f64,
    },
    #[serde(rename = "minecraft:y_clamped_gradient")]
    YClampedGradient {
        from_y: i32,
        to_y: i32,
        from_value: f64,
        to_value: f64,
    },
    #[serde(rename = "minecraft:noise")]
    Noise {
        xz_scale: f64,
        y_scale: f64,
        noise: String,
    },
    #[serde(rename = "minecraft:shifted_noise")]
    ShiftedNoise {
        shift_x: Box<DensityFunctionJson>,
        shift_y: Box<DensityFunctionJson>,
        shift_z: Box<DensityFunctionJson>,
        xz_scale: f64,
        y_scale: f64,
        noise: String,
    },
    #[serde(rename = "minecraft:shift_a")]
    ShiftA {
        #[serde(rename = "argument")]
        noise: String,
    },
    #[serde(rename = "minecraft:shift_b")]
    ShiftB {
        #[serde(rename = "argument")]
        noise: String,
    },
    #[serde(rename = "minecraft:shift")]
    Shift {
        #[serde(rename = "argument")]
        noise: String,
    },
    #[serde(rename = "minecraft:clamp")]
    Clamp {
        input: Box<DensityFunctionJson>,
        min: f64,
        max: f64,
    },
    #[serde(rename = "minecraft:abs")]
    Abs {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:square")]
    Square {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:cube")]
    Cube {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:half_negative")]
    HalfNegative {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:quarter_negative")]
    QuarterNegative {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:invert")]
    Invert {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:squeeze")]
    Squeeze {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:add")]
    Add {
        argument1: Box<DensityFunctionJson>,
        argument2: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:mul")]
    Mul {
        argument1: Box<DensityFunctionJson>,
        argument2: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:min")]
    Min {
        argument1: Box<DensityFunctionJson>,
        argument2: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:max")]
    Max {
        argument1: Box<DensityFunctionJson>,
        argument2: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:spline")]
    Spline { spline: SplineJson },
    #[serde(rename = "minecraft:range_choice")]
    RangeChoice {
        input: Box<DensityFunctionJson>,
        min_inclusive: f64,
        max_exclusive: f64,
        when_in_range: Box<DensityFunctionJson>,
        when_out_of_range: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:interpolated")]
    Interpolated { argument: Box<DensityFunctionJson> },
    #[serde(rename = "minecraft:flat_cache")]
    FlatCache { argument: Box<DensityFunctionJson> },
    #[serde(rename = "minecraft:cache_once")]
    CacheOnce { argument: Box<DensityFunctionJson> },
    #[serde(rename = "minecraft:cache_2d")]
    Cache2d { argument: Box<DensityFunctionJson> },
    #[serde(rename = "minecraft:cache_all_in_cell")]
    CacheAllInCell { argument: Box<DensityFunctionJson> },
    #[serde(rename = "minecraft:blend_offset")]
    BlendOffset {},
    #[serde(rename = "minecraft:blend_alpha")]
    BlendAlpha {},
    #[serde(rename = "minecraft:blend_density")]
    BlendDensity {
        #[serde(rename = "argument")]
        input: Box<DensityFunctionJson>,
    },
    #[serde(rename = "minecraft:beardifier")]
    Beardifier {},
    #[serde(rename = "minecraft:end_islands")]
    EndIslands {},
    #[serde(rename = "minecraft:weird_scaled_sampler")]
    WeirdScaledSampler {
        input: Box<DensityFunctionJson>,
        noise: String,
        rarity_value_mapper: String,
    },
    #[serde(rename = "minecraft:old_blended_noise")]
    OldBlendedNoise {
        xz_scale: f64,
        y_scale: f64,
        xz_factor: f64,
        y_factor: f64,
        smear_scale_multiplier: f64,
    },
    /// find_top_surface is only used in preliminary_surface_level which is unused
    #[serde(rename = "minecraft:find_top_surface")]
    FindTopSurface {},
}
/// Parsed spline from datapack JSON.
///
/// In the datapack format, a spline value can be:
/// - A bare number -> Constant
/// - An object with {coordinate, points} -> Multipoint
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SplineJson {
    Constant(f32),
    Multipoint {
        coordinate: String,
        #[serde(default)]
        points: Vec<SplinePointJson>,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct SplinePointJson {
    pub location: f32,
    pub value: SplineJson,
    pub derivative: f32,
}

/// Parsed noise router from a noise_settings datapack file.
#[derive(Deserialize)]
pub struct NoiseRouterJson {
    barrier: DensityFunctionJson,
    fluid_level_floodedness: DensityFunctionJson,
    fluid_level_spread: DensityFunctionJson,
    lava: DensityFunctionJson,
    temperature: DensityFunctionJson,
    vegetation: DensityFunctionJson,
    continents: DensityFunctionJson,
    erosion: DensityFunctionJson,
    depth: DensityFunctionJson,
    ridges: DensityFunctionJson,
    preliminary_surface_level: Option<DensityFunctionJson>,
    final_density: DensityFunctionJson,
    vein_toggle: DensityFunctionJson,
    vein_ridged: DensityFunctionJson,
    vein_gap: DensityFunctionJson,
}

/// Wrapper for deserializing noise settings files that contain a `noise_router` field.
#[derive(Deserialize)]
struct NoiseSettingsJson {
    noise_router: NoiseRouterJson,
}

// ── Datapack file reading ───────────────────────────────────────────────────

const DATAPACK_BASE: &str = "build_assets/builtin_datapacks/minecraft/data/minecraft/worldgen";

/// Recursively collect all .json files under a directory.
fn collect_json_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(collect_json_files(&path));
            } else if path.extension().is_some_and(|ext| ext == "json") {
                files.push(path);
            }
        }
    }
    files
}

/// Convert a density_function file path to a registry ID.
///
/// e.g. `.../density_function/overworld/continents.json` -> `minecraft:overworld/continents`
fn path_to_id(path: &Path, base_dir: &Path) -> String {
    let relative = path.strip_prefix(base_dir).unwrap();
    let without_ext = relative.with_extension("");
    // Convert OS path separators to forward slashes
    let id_path = without_ext
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/");
    format!("minecraft:{id_path}")
}

/// Read all density function files from the datapack into a registry.
fn read_density_function_registry() -> BTreeMap<String, DensityFunctionJson> {
    let df_dir = format!("{DATAPACK_BASE}/density_function");
    let df_path = Path::new(&df_dir);
    let mut registry = BTreeMap::new();

    for file in collect_json_files(df_path) {
        println!("cargo:rerun-if-changed={}", file.display());
        let content = fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", file.display()));
        let df: DensityFunctionJson = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", file.display()));
        let id = path_to_id(&file, df_path);
        registry.insert(id, df);
    }

    registry
}

/// Read the overworld noise settings from the datapack.
fn read_overworld_noise_router() -> NoiseRouterJson {
    let path = format!("{DATAPACK_BASE}/noise_settings/overworld.json");
    println!("cargo:rerun-if-changed={path}");
    let content =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let settings: NoiseSettingsJson =
        serde_json::from_str(&content).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    settings.noise_router
}

// ── Code generation using runtime types ─────────────────────────────────────

/// Generate the complete density functions module.
pub(crate) fn build() -> TokenStream {
    let router = read_overworld_noise_router();
    let registry = read_density_function_registry();

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated density function data for terrain generation.
        //!
        //! This file is auto-generated from the vanilla datapack files.
        //! Do not edit manually.
        //!
        //! Types are imported from `steel_utils::density` — only data is generated.

        use std::sync::{Arc, LazyLock};
        use steel_utils::density::*;
    });

    stream.extend(generate_noise_router_instance(&router));
    stream.extend(generate_density_function_registry(&registry));

    stream
}

fn generate_noise_router_instance(router: &NoiseRouterJson) -> TokenStream {
    let barrier = gen_df(&router.barrier);
    let fluid_floodedness = gen_df(&router.fluid_level_floodedness);
    let fluid_spread = gen_df(&router.fluid_level_spread);
    let lava = gen_df(&router.lava);
    let temperature = gen_df(&router.temperature);
    let vegetation = gen_df(&router.vegetation);
    let continents = gen_df(&router.continents);
    let erosion = gen_df(&router.erosion);
    let depth = gen_df(&router.depth);
    let ridges = gen_df(&router.ridges);
    let final_density = gen_df(&router.final_density);
    let vein_toggle = gen_df(&router.vein_toggle);
    let vein_ridged = gen_df(&router.vein_ridged);
    let vein_gap = gen_df(&router.vein_gap);

    let preliminary = if let Some(ref psl) = router.preliminary_surface_level {
        let code = gen_df(psl);
        quote! { #code }
    } else {
        quote! { Arc::new(DensityFunction::constant(0.0)) }
    };

    quote! {
        /// Overworld noise router with all density functions (unresolved).
        pub static OVERWORLD_NOISE_ROUTER: LazyLock<NoiseRouter> = LazyLock::new(|| {
            NoiseRouter {
                barrier_noise: #barrier,
                fluid_level_floodedness: #fluid_floodedness,
                fluid_level_spread: #fluid_spread,
                lava: #lava,
                temperature: #temperature,
                vegetation: #vegetation,
                continentalness: #continents,
                erosion: #erosion,
                depth: #depth,
                ridges: #ridges,
                preliminary_surface_level: #preliminary,
                final_density: #final_density,
                vein_toggle: #vein_toggle,
                vein_ridged: #vein_ridged,
                vein_gap: #vein_gap,
            }
        });
    }
}

/// Generate a `TokenStream` that constructs an `Arc<DensityFunction>`.
fn gen_df(json: &DensityFunctionJson) -> TokenStream {
    match json {
        DensityFunctionJson::Constant(value) => {
            quote! { Arc::new(DensityFunction::Constant(Constant { value: #value })) }
        }
        DensityFunctionJson::Reference(id) => {
            quote! { Arc::new(DensityFunction::Reference(Reference { id: #id.to_string(), resolved: None })) }
        }
        DensityFunctionJson::Data(data) => gen_data(data),
    }
}

fn gen_data(data: &DensityFunctionData) -> TokenStream {
    match data {
        DensityFunctionData::Constant { value } => {
            quote! { Arc::new(DensityFunction::Constant(Constant { value: #value })) }
        }

        DensityFunctionData::YClampedGradient {
            from_y,
            to_y,
            from_value,
            to_value,
        } => {
            quote! { Arc::new(DensityFunction::YClampedGradient(YClampedGradient {
                from_y: #from_y, to_y: #to_y, from_value: #from_value, to_value: #to_value,
            })) }
        }

        DensityFunctionData::Noise {
            xz_scale,
            y_scale,
            noise,
        } => {
            quote! { Arc::new(DensityFunction::Noise(Noise {
                noise_id: #noise.to_string(), xz_scale: #xz_scale, y_scale: #y_scale, noise: None,
            })) }
        }

        DensityFunctionData::ShiftedNoise {
            shift_x,
            shift_y,
            shift_z,
            xz_scale,
            y_scale,
            noise,
        } => {
            let sx = gen_df(shift_x);
            let sy = gen_df(shift_y);
            let sz = gen_df(shift_z);
            quote! { Arc::new(DensityFunction::ShiftedNoise(ShiftedNoise {
                shift_x: #sx, shift_y: #sy, shift_z: #sz,
                xz_scale: #xz_scale, y_scale: #y_scale,
                noise_id: #noise.to_string(), noise: None,
            })) }
        }

        DensityFunctionData::ShiftA { noise } => {
            quote! { Arc::new(DensityFunction::ShiftA(ShiftA { noise_id: #noise.to_string(), noise: None })) }
        }
        DensityFunctionData::ShiftB { noise } => {
            quote! { Arc::new(DensityFunction::ShiftB(ShiftB { noise_id: #noise.to_string(), noise: None })) }
        }
        DensityFunctionData::Shift { noise } => {
            quote! { Arc::new(DensityFunction::Shift(Shift { noise_id: #noise.to_string(), noise: None })) }
        }

        DensityFunctionData::Clamp { input, min, max } => {
            let input_code = gen_df(input);
            quote! { Arc::new(DensityFunction::Clamp(Clamp { input: #input_code, min: #min, max: #max })) }
        }

        DensityFunctionData::Abs { input } => gen_mapped(MappedTypeKind::Abs, input),
        DensityFunctionData::Square { input } => gen_mapped(MappedTypeKind::Square, input),
        DensityFunctionData::Cube { input } => gen_mapped(MappedTypeKind::Cube, input),
        DensityFunctionData::HalfNegative { input } => {
            gen_mapped(MappedTypeKind::HalfNegative, input)
        }
        DensityFunctionData::QuarterNegative { input } => {
            gen_mapped(MappedTypeKind::QuarterNegative, input)
        }
        DensityFunctionData::Invert { input } => gen_mapped(MappedTypeKind::Invert, input),
        DensityFunctionData::Squeeze { input } => gen_mapped(MappedTypeKind::Squeeze, input),

        DensityFunctionData::Add {
            argument1,
            argument2,
        } => gen_two_arg(TwoArgKind::Add, argument1, argument2),
        DensityFunctionData::Mul {
            argument1,
            argument2,
        } => gen_two_arg(TwoArgKind::Mul, argument1, argument2),
        DensityFunctionData::Min {
            argument1,
            argument2,
        } => gen_two_arg(TwoArgKind::Min, argument1, argument2),
        DensityFunctionData::Max {
            argument1,
            argument2,
        } => gen_two_arg(TwoArgKind::Max, argument1, argument2),

        DensityFunctionData::Spline { spline } => {
            let spline_code = gen_spline(spline);
            quote! { Arc::new(DensityFunction::Spline(Spline { spline: Arc::new(#spline_code) })) }
        }

        DensityFunctionData::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        } => {
            let input_code = gen_df(input);
            let in_range = gen_df(when_in_range);
            let out_range = gen_df(when_out_of_range);
            quote! { Arc::new(DensityFunction::RangeChoice(RangeChoice {
                input: #input_code, min_inclusive: #min_inclusive, max_exclusive: #max_exclusive,
                when_in_range: #in_range, when_out_of_range: #out_range,
            })) }
        }

        DensityFunctionData::Interpolated { argument } => {
            gen_marker(MarkerKind::Interpolated, argument)
        }
        DensityFunctionData::FlatCache { argument } => gen_marker(MarkerKind::FlatCache, argument),
        DensityFunctionData::CacheOnce { argument } => gen_marker(MarkerKind::CacheOnce, argument),
        DensityFunctionData::Cache2d { argument } => gen_marker(MarkerKind::Cache2D, argument),
        DensityFunctionData::CacheAllInCell { argument } => {
            gen_marker(MarkerKind::CacheAllInCell, argument)
        }

        DensityFunctionData::BlendOffset {} => {
            quote! { Arc::new(DensityFunction::BlendOffset(BlendOffset)) }
        }
        DensityFunctionData::BlendAlpha {} => {
            quote! { Arc::new(DensityFunction::BlendAlpha(BlendAlpha)) }
        }
        DensityFunctionData::BlendDensity { input } => {
            let input_code = gen_df(input);
            quote! { Arc::new(DensityFunction::BlendDensity(BlendDensity { input: #input_code })) }
        }

        // TODO: Implement Beardifier for structure terrain adaptation.
        // Constant(0.0) is correct when structures are not yet generated.
        DensityFunctionData::Beardifier {} => {
            quote! { Arc::new(DensityFunction::Constant(Constant { value: 0.0 })) }
        }
        DensityFunctionData::EndIslands {} => {
            quote! { Arc::new(DensityFunction::EndIslands(EndIslands)) }
        }

        DensityFunctionData::WeirdScaledSampler {
            input,
            noise,
            rarity_value_mapper,
        } => {
            let input_code = gen_df(input);
            let mapper = match rarity_value_mapper.as_str() {
                "type_1" => quote! { RarityValueMapper::Tunnels },
                _ => quote! { RarityValueMapper::Caves },
            };
            quote! { Arc::new(DensityFunction::WeirdScaledSampler(WeirdScaledSampler {
                input: #input_code, noise_id: #noise.to_string(),
                rarity_value_mapper: #mapper, noise: None,
            })) }
        }

        DensityFunctionData::OldBlendedNoise {
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        } => {
            quote! { Arc::new(DensityFunction::BlendedNoise(BlendedNoise {
                xz_scale: #xz_scale, y_scale: #y_scale, xz_factor: #xz_factor,
                y_factor: #y_factor, smear_scale_multiplier: #smear_scale_multiplier, noise: None,
            })) }
        }

        DensityFunctionData::FindTopSurface {} => {
            // find_top_surface is unused; treat as constant 0
            quote! { Arc::new(DensityFunction::Constant(Constant { value: 0.0 })) }
        }
    }
}

// Helper enums to avoid importing runtime types in the build script

enum MappedTypeKind {
    Abs,
    Square,
    Cube,
    HalfNegative,
    QuarterNegative,
    Invert,
    Squeeze,
}
enum TwoArgKind {
    Add,
    Mul,
    Min,
    Max,
}
enum MarkerKind {
    Interpolated,
    FlatCache,
    CacheOnce,
    Cache2D,
    CacheAllInCell,
}

fn gen_mapped(kind: MappedTypeKind, input: &DensityFunctionJson) -> TokenStream {
    let input_code = gen_df(input);
    let op = match kind {
        MappedTypeKind::Abs => quote! { MappedType::Abs },
        MappedTypeKind::Square => quote! { MappedType::Square },
        MappedTypeKind::Cube => quote! { MappedType::Cube },
        MappedTypeKind::HalfNegative => quote! { MappedType::HalfNegative },
        MappedTypeKind::QuarterNegative => quote! { MappedType::QuarterNegative },
        MappedTypeKind::Invert => quote! { MappedType::Invert },
        MappedTypeKind::Squeeze => quote! { MappedType::Squeeze },
    };
    quote! { Arc::new(DensityFunction::Mapped(Mapped { op: #op, input: #input_code })) }
}

fn gen_two_arg(kind: TwoArgKind, a: &DensityFunctionJson, b: &DensityFunctionJson) -> TokenStream {
    let a_code = gen_df(a);
    let b_code = gen_df(b);
    let op = match kind {
        TwoArgKind::Add => quote! { TwoArgType::Add },
        TwoArgKind::Mul => quote! { TwoArgType::Mul },
        TwoArgKind::Min => quote! { TwoArgType::Min },
        TwoArgKind::Max => quote! { TwoArgType::Max },
    };
    quote! { Arc::new(DensityFunction::TwoArgumentSimple(TwoArgumentSimple { op: #op, argument1: #a_code, argument2: #b_code })) }
}

fn gen_marker(kind: MarkerKind, argument: &DensityFunctionJson) -> TokenStream {
    let arg_code = gen_df(argument);
    let mk = match kind {
        MarkerKind::Interpolated => quote! { MarkerType::Interpolated },
        MarkerKind::FlatCache => quote! { MarkerType::FlatCache },
        MarkerKind::CacheOnce => quote! { MarkerType::CacheOnce },
        MarkerKind::Cache2D => quote! { MarkerType::Cache2D },
        MarkerKind::CacheAllInCell => quote! { MarkerType::CacheAllInCell },
    };
    quote! { Arc::new(DensityFunction::Marker(Marker { kind: #mk, wrapped: #arg_code })) }
}

fn gen_spline(spline: &SplineJson) -> TokenStream {
    match spline {
        SplineJson::Constant(v) => {
            quote! { CubicSpline {
                coordinate: Arc::new(DensityFunction::constant(0.0)),
                points: vec![SplinePoint { location: 0.0, value: SplineValue::Constant(#v), derivative: 0.0 }],
            } }
        }
        SplineJson::Multipoint { coordinate, points } => {
            let points_code: Vec<TokenStream> = points
                .iter()
                .map(|p| {
                    let loc = p.location;
                    let val = gen_spline_value(&p.value);
                    let deriv = p.derivative;
                    quote! { SplinePoint { location: #loc, value: #val, derivative: #deriv } }
                })
                .collect();
            quote! { CubicSpline {
                coordinate: Arc::new(DensityFunction::Reference(Reference { id: #coordinate.to_string(), resolved: None })),
                points: vec![#(#points_code),*],
            } }
        }
    }
}

fn gen_spline_value(spline: &SplineJson) -> TokenStream {
    match spline {
        SplineJson::Constant(v) => quote! { SplineValue::Constant(#v) },
        SplineJson::Multipoint { .. } => {
            let spline_code = gen_spline(spline);
            quote! { SplineValue::Spline(Arc::new(#spline_code)) }
        }
    }
}

fn generate_density_function_registry(
    registry: &BTreeMap<String, DensityFunctionJson>,
) -> TokenStream {
    let entries: Vec<TokenStream> = registry
        .iter()
        .map(|(id, df)| {
            let df_code = gen_df(df);
            quote! { #id => Some(#df_code) }
        })
        .collect();

    quote! {
        /// Get density function by ID from the registry.
        pub fn get_density_function(id: &str) -> Option<Arc<DensityFunction>> {
            match id {
                #(#entries,)*
                _ => None,
            }
        }
    }
}
