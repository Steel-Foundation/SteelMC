use proc_macro2::{Ident, Span, TokenStream};
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

// ── Code generation ─────────────────────────────────────────────────────────

/// Generate the Rust code for density functions.
pub(crate) fn build() -> TokenStream {
    let router = read_overworld_noise_router();
    let registry = read_density_function_registry();

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        //! Generated density functions and noise routers.
        //!
        //! This file is auto-generated from the vanilla datapack files.
        //! Do not edit manually.

        use std::sync::Arc;
    });

    stream.extend(generate_density_function_enum());
    stream.extend(generate_spline_types());
    stream.extend(generate_noise_router_struct());

    let router_code = generate_noise_router_instance("OVERWORLD", &router);
    stream.extend(router_code);

    let registry_code = generate_density_function_registry(&registry);
    stream.extend(registry_code);

    stream
}

fn generate_density_function_enum() -> TokenStream {
    quote! {
        /// Density function types for terrain generation.
        #[derive(Debug, Clone)]
        pub enum DensityFunction {
            /// Constant value
            Constant(f64),

            /// Reference to another density function by ID
            Reference(&'static str),

            /// Y clamped gradient
            YClampedGradient {
                from_y: i32,
                to_y: i32,
                from_value: f64,
                to_value: f64,
            },

            /// Noise sampler
            Noise {
                noise_id: &'static str,
                xz_scale: f64,
                y_scale: f64,
            },

            /// Shifted noise sampler
            ShiftedNoise {
                shift_x: Arc<DensityFunction>,
                shift_y: Arc<DensityFunction>,
                shift_z: Arc<DensityFunction>,
                xz_scale: f64,
                y_scale: f64,
                noise_id: &'static str,
            },

            /// Shift A (for offset noise)
            ShiftA { noise_id: &'static str },
            /// Shift B (for offset noise)
            ShiftB { noise_id: &'static str },
            /// Shift (for offset noise)
            Shift { noise_id: &'static str },

            /// Clamp value to range
            Clamp {
                input: Arc<DensityFunction>,
                min: f64,
                max: f64,
            },

            /// Absolute value
            Abs(Arc<DensityFunction>),
            /// Square
            Square(Arc<DensityFunction>),
            /// Cube
            Cube(Arc<DensityFunction>),
            /// Half negative (v > 0 ? v : v * 0.5)
            HalfNegative(Arc<DensityFunction>),
            /// Quarter negative (v > 0 ? v : v * 0.25)
            QuarterNegative(Arc<DensityFunction>),
            /// Squeeze function
            Squeeze(Arc<DensityFunction>),

            /// Add two functions
            Add(Arc<DensityFunction>, Arc<DensityFunction>),
            /// Multiply two functions
            Mul(Arc<DensityFunction>, Arc<DensityFunction>),
            /// Minimum of two functions
            Min(Arc<DensityFunction>, Arc<DensityFunction>),
            /// Maximum of two functions
            Max(Arc<DensityFunction>, Arc<DensityFunction>),

            /// Cubic spline
            Spline(CubicSpline),

            /// Range choice
            RangeChoice {
                input: Arc<DensityFunction>,
                min_inclusive: f64,
                max_exclusive: f64,
                when_in_range: Arc<DensityFunction>,
                when_out_of_range: Arc<DensityFunction>,
            },

            /// Interpolated (cache marker)
            Interpolated(Arc<DensityFunction>),
            /// Flat cache (cache marker)
            FlatCache(Arc<DensityFunction>),
            /// Cache once
            CacheOnce(Arc<DensityFunction>),
            /// Cache 2D
            Cache2d(Arc<DensityFunction>),
            /// Cache all in cell
            CacheAllInCell(Arc<DensityFunction>),

            /// Blend offset (for world blending)
            BlendOffset,
            /// Blend alpha (for world blending)
            BlendAlpha,
            /// Blend density
            BlendDensity(Arc<DensityFunction>),

            /// Beardifier (structure terrain modification)
            Beardifier,

            /// End islands
            EndIslands,

            /// Weird scaled sampler
            WeirdScaledSampler {
                input: Arc<DensityFunction>,
                noise_id: &'static str,
                rarity_value_mapper: RarityValueMapper,
            },

            /// Old blended noise (3D terrain noise)
            OldBlendedNoise {
                xz_scale: f64,
                y_scale: f64,
                xz_factor: f64,
                y_factor: f64,
                smear_scale_multiplier: f64,
            },
        }

        /// Rarity value mapper for weird scaled sampler
        #[derive(Debug, Clone, Copy)]
        pub enum RarityValueMapper {
            Tunnels,
            Caves,
        }
    }
}

fn generate_spline_types() -> TokenStream {
    quote! {
        /// A point in a cubic spline
        #[derive(Debug, Clone)]
        pub struct SplinePoint {
            pub location: f32,
            pub value: CubicSpline,
            pub derivative: f32,
        }

        /// Cubic spline for density function interpolation
        #[derive(Debug, Clone)]
        pub enum CubicSpline {
            /// Constant value
            Constant(f32),
            /// Multipoint spline with coordinate function and points
            Multipoint {
                coordinate: Arc<DensityFunction>,
                points: Vec<SplinePoint>,
            },
        }
    }
}

fn generate_noise_router_struct() -> TokenStream {
    quote! {
        /// Noise router containing all density functions for terrain generation.
        #[derive(Debug, Clone)]
        pub struct NoiseRouter {
            pub barrier: Arc<DensityFunction>,
            pub fluid_level_floodedness: Arc<DensityFunction>,
            pub fluid_level_spread: Arc<DensityFunction>,
            pub lava: Arc<DensityFunction>,
            pub temperature: Arc<DensityFunction>,
            pub vegetation: Arc<DensityFunction>,
            pub continents: Arc<DensityFunction>,
            pub erosion: Arc<DensityFunction>,
            pub depth: Arc<DensityFunction>,
            pub ridges: Arc<DensityFunction>,
            pub preliminary_surface_level: Option<Arc<DensityFunction>>,
            pub final_density: Arc<DensityFunction>,
            pub vein_toggle: Arc<DensityFunction>,
            pub vein_ridged: Arc<DensityFunction>,
            pub vein_gap: Arc<DensityFunction>,
        }
    }
}

fn generate_noise_router_instance(name: &str, router: &NoiseRouterJson) -> TokenStream {
    let name_ident = Ident::new(&format!("{}_NOISE_ROUTER", name), Span::call_site());

    let barrier = router.barrier.generate_code();
    let fluid_floodedness = router.fluid_level_floodedness.generate_code();
    let fluid_spread = router.fluid_level_spread.generate_code();
    let lava = router.lava.generate_code();
    let temperature = router.temperature.generate_code();
    let vegetation = router.vegetation.generate_code();
    let continents = router.continents.generate_code();
    let erosion = router.erosion.generate_code();
    let depth = router.depth.generate_code();
    let ridges = router.ridges.generate_code();
    let final_density = router.final_density.generate_code();
    let vein_toggle = router.vein_toggle.generate_code();
    let vein_ridged = router.vein_ridged.generate_code();
    let vein_gap = router.vein_gap.generate_code();

    let preliminary = if let Some(ref psl) = router.preliminary_surface_level {
        let code = psl.generate_code();
        quote! { Some(Arc::new(#code)) }
    } else {
        quote! { None }
    };

    quote! {
        use std::sync::LazyLock;

        /// Overworld noise router with all density functions.
        pub static #name_ident: LazyLock<NoiseRouter> = LazyLock::new(|| {
            NoiseRouter {
                barrier: Arc::new(#barrier),
                fluid_level_floodedness: Arc::new(#fluid_floodedness),
                fluid_level_spread: Arc::new(#fluid_spread),
                lava: Arc::new(#lava),
                temperature: Arc::new(#temperature),
                vegetation: Arc::new(#vegetation),
                continents: Arc::new(#continents),
                erosion: Arc::new(#erosion),
                depth: Arc::new(#depth),
                ridges: Arc::new(#ridges),
                preliminary_surface_level: #preliminary,
                final_density: Arc::new(#final_density),
                vein_toggle: Arc::new(#vein_toggle),
                vein_ridged: Arc::new(#vein_ridged),
                vein_gap: Arc::new(#vein_gap),
            }
        });
    }
}

/// Trait for generating `TokenStream` code from parsed datapack types.
trait GenerateCode {
    fn generate_code(&self) -> TokenStream;
}

impl GenerateCode for DensityFunctionJson {
    fn generate_code(&self) -> TokenStream {
        match self {
            DensityFunctionJson::Constant(value) => {
                quote! { DensityFunction::Constant(#value) }
            }
            DensityFunctionJson::Reference(id) => {
                quote! { DensityFunction::Reference(#id) }
            }
            DensityFunctionJson::Data(data) => data.generate_code(),
        }
    }
}

impl GenerateCode for DensityFunctionData {
    fn generate_code(&self) -> TokenStream {
        match self {
            DensityFunctionData::Constant { value } => {
                quote! { DensityFunction::Constant(#value) }
            }

            DensityFunctionData::YClampedGradient {
                from_y,
                to_y,
                from_value,
                to_value,
            } => {
                quote! {
                    DensityFunction::YClampedGradient {
                        from_y: #from_y,
                        to_y: #to_y,
                        from_value: #from_value,
                        to_value: #to_value,
                    }
                }
            }

            DensityFunctionData::Noise {
                xz_scale,
                y_scale,
                noise,
            } => {
                quote! {
                    DensityFunction::Noise {
                        noise_id: #noise,
                        xz_scale: #xz_scale,
                        y_scale: #y_scale,
                    }
                }
            }

            DensityFunctionData::ShiftedNoise {
                shift_x,
                shift_y,
                shift_z,
                xz_scale,
                y_scale,
                noise,
            } => {
                let shift_x_code = shift_x.generate_code();
                let shift_y_code = shift_y.generate_code();
                let shift_z_code = shift_z.generate_code();
                quote! {
                    DensityFunction::ShiftedNoise {
                        shift_x: Arc::new(#shift_x_code),
                        shift_y: Arc::new(#shift_y_code),
                        shift_z: Arc::new(#shift_z_code),
                        xz_scale: #xz_scale,
                        y_scale: #y_scale,
                        noise_id: #noise,
                    }
                }
            }

            DensityFunctionData::ShiftA { noise } => {
                quote! { DensityFunction::ShiftA { noise_id: #noise } }
            }

            DensityFunctionData::ShiftB { noise } => {
                quote! { DensityFunction::ShiftB { noise_id: #noise } }
            }

            DensityFunctionData::Shift { noise } => {
                quote! { DensityFunction::Shift { noise_id: #noise } }
            }

            DensityFunctionData::Clamp { input, min, max } => {
                let input_code = input.generate_code();
                quote! {
                    DensityFunction::Clamp {
                        input: Arc::new(#input_code),
                        min: #min,
                        max: #max,
                    }
                }
            }

            DensityFunctionData::Abs { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::Abs(Arc::new(#input_code)) }
            }

            DensityFunctionData::Square { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::Square(Arc::new(#input_code)) }
            }

            DensityFunctionData::Cube { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::Cube(Arc::new(#input_code)) }
            }

            DensityFunctionData::HalfNegative { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::HalfNegative(Arc::new(#input_code)) }
            }

            DensityFunctionData::QuarterNegative { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::QuarterNegative(Arc::new(#input_code)) }
            }

            DensityFunctionData::Squeeze { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::Squeeze(Arc::new(#input_code)) }
            }

            DensityFunctionData::Add {
                argument1,
                argument2,
            } => {
                let arg1 = argument1.generate_code();
                let arg2 = argument2.generate_code();
                quote! { DensityFunction::Add(Arc::new(#arg1), Arc::new(#arg2)) }
            }

            DensityFunctionData::Mul {
                argument1,
                argument2,
            } => {
                let arg1 = argument1.generate_code();
                let arg2 = argument2.generate_code();
                quote! { DensityFunction::Mul(Arc::new(#arg1), Arc::new(#arg2)) }
            }

            DensityFunctionData::Min {
                argument1,
                argument2,
            } => {
                let arg1 = argument1.generate_code();
                let arg2 = argument2.generate_code();
                quote! { DensityFunction::Min(Arc::new(#arg1), Arc::new(#arg2)) }
            }

            DensityFunctionData::Max {
                argument1,
                argument2,
            } => {
                let arg1 = argument1.generate_code();
                let arg2 = argument2.generate_code();
                quote! { DensityFunction::Max(Arc::new(#arg1), Arc::new(#arg2)) }
            }

            DensityFunctionData::Spline { spline } => {
                let spline_code = spline.generate_code();
                quote! { DensityFunction::Spline(#spline_code) }
            }

            DensityFunctionData::RangeChoice {
                input,
                min_inclusive,
                max_exclusive,
                when_in_range,
                when_out_of_range,
            } => {
                let input_code = input.generate_code();
                let in_range_code = when_in_range.generate_code();
                let out_of_range_code = when_out_of_range.generate_code();
                quote! {
                    DensityFunction::RangeChoice {
                        input: Arc::new(#input_code),
                        min_inclusive: #min_inclusive,
                        max_exclusive: #max_exclusive,
                        when_in_range: Arc::new(#in_range_code),
                        when_out_of_range: Arc::new(#out_of_range_code),
                    }
                }
            }

            DensityFunctionData::Interpolated { argument } => {
                let arg_code = argument.generate_code();
                quote! { DensityFunction::Interpolated(Arc::new(#arg_code)) }
            }

            DensityFunctionData::FlatCache { argument } => {
                let arg_code = argument.generate_code();
                quote! { DensityFunction::FlatCache(Arc::new(#arg_code)) }
            }

            DensityFunctionData::CacheOnce { argument } => {
                let arg_code = argument.generate_code();
                quote! { DensityFunction::CacheOnce(Arc::new(#arg_code)) }
            }

            DensityFunctionData::Cache2d { argument } => {
                let arg_code = argument.generate_code();
                quote! { DensityFunction::Cache2d(Arc::new(#arg_code)) }
            }

            DensityFunctionData::CacheAllInCell { argument } => {
                let arg_code = argument.generate_code();
                quote! { DensityFunction::CacheAllInCell(Arc::new(#arg_code)) }
            }

            DensityFunctionData::BlendOffset {} => {
                quote! { DensityFunction::BlendOffset }
            }

            DensityFunctionData::BlendAlpha {} => {
                quote! { DensityFunction::BlendAlpha }
            }

            DensityFunctionData::BlendDensity { input } => {
                let input_code = input.generate_code();
                quote! { DensityFunction::BlendDensity(Arc::new(#input_code)) }
            }

            DensityFunctionData::Beardifier {} => {
                quote! { DensityFunction::Beardifier }
            }

            DensityFunctionData::EndIslands {} => {
                quote! { DensityFunction::EndIslands }
            }

            DensityFunctionData::WeirdScaledSampler {
                input,
                noise,
                rarity_value_mapper,
            } => {
                let input_code = input.generate_code();
                let mapper = match rarity_value_mapper.as_str() {
                    "type_1" => quote! { RarityValueMapper::Tunnels },
                    "type_2" => quote! { RarityValueMapper::Caves },
                    _ => quote! { RarityValueMapper::Caves },
                };
                quote! {
                    DensityFunction::WeirdScaledSampler {
                        input: Arc::new(#input_code),
                        noise_id: #noise,
                        rarity_value_mapper: #mapper,
                    }
                }
            }

            DensityFunctionData::OldBlendedNoise {
                xz_scale,
                y_scale,
                xz_factor,
                y_factor,
                smear_scale_multiplier,
            } => {
                quote! {
                    DensityFunction::OldBlendedNoise {
                        xz_scale: #xz_scale,
                        y_scale: #y_scale,
                        xz_factor: #xz_factor,
                        y_factor: #y_factor,
                        smear_scale_multiplier: #smear_scale_multiplier,
                    }
                }
            }

            DensityFunctionData::FindTopSurface {} => {
                // find_top_surface is unused; treat as constant 0
                quote! { DensityFunction::Constant(0.0) }
            }
        }
    }
}

impl GenerateCode for SplineJson {
    fn generate_code(&self) -> TokenStream {
        match self {
            SplineJson::Constant(value) => {
                let v = *value;
                quote! { CubicSpline::Constant(#v) }
            }

            SplineJson::Multipoint { coordinate, points } => {
                let coord_code = quote! { Arc::new(DensityFunction::Reference(#coordinate)) };

                let points_code: Vec<TokenStream> = points
                    .iter()
                    .map(|p| {
                        let loc = p.location;
                        let val_code = p.value.generate_code();
                        let deriv = p.derivative;
                        quote! {
                            SplinePoint {
                                location: #loc,
                                value: #val_code,
                                derivative: #deriv,
                            }
                        }
                    })
                    .collect();

                quote! {
                    CubicSpline::Multipoint {
                        coordinate: #coord_code,
                        points: vec![#(#points_code),*],
                    }
                }
            }
        }
    }
}

fn generate_density_function_registry(
    registry: &BTreeMap<String, DensityFunctionJson>,
) -> TokenStream {
    let entries: Vec<TokenStream> = registry
        .iter()
        .map(|(id, df)| {
            let df_code = df.generate_code();
            quote! { #id => Some(Arc::new(#df_code)) }
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
