use proc_macro2::TokenStream;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
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

// ── JSON → DensityFunction conversion ───────────────────────────────────────

use steel_utils::density::*;

/// Convert a JSON density function to a runtime `DensityFunction` value.
///
/// Noises are left as `None` (baked at runtime from seed).
/// References are left unresolved (the transpiler handles them via the registry).
fn json_to_df(json: &DensityFunctionJson) -> DensityFunction {
    match json {
        DensityFunctionJson::Constant(value) => {
            DensityFunction::Constant(Constant { value: *value })
        }
        DensityFunctionJson::Reference(id) => DensityFunction::Reference(Reference {
            id: id.clone(),
            resolved: None,
        }),
        DensityFunctionJson::Data(data) => json_data_to_df(data),
    }
}

fn json_data_to_df(data: &DensityFunctionData) -> DensityFunction {
    match data {
        DensityFunctionData::Constant { value } => {
            DensityFunction::Constant(Constant { value: *value })
        }

        DensityFunctionData::YClampedGradient {
            from_y,
            to_y,
            from_value,
            to_value,
        } => DensityFunction::YClampedGradient(YClampedGradient {
            from_y: *from_y,
            to_y: *to_y,
            from_value: *from_value,
            to_value: *to_value,
        }),

        DensityFunctionData::Noise {
            xz_scale,
            y_scale,
            noise,
        } => DensityFunction::Noise(Noise {
            noise_id: noise.clone(),
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            noise: None,
        }),

        DensityFunctionData::ShiftedNoise {
            shift_x,
            shift_y,
            shift_z,
            xz_scale,
            y_scale,
            noise,
        } => DensityFunction::ShiftedNoise(ShiftedNoise {
            shift_x: Arc::new(json_to_df(shift_x)),
            shift_y: Arc::new(json_to_df(shift_y)),
            shift_z: Arc::new(json_to_df(shift_z)),
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            noise_id: noise.clone(),
            noise: None,
        }),

        DensityFunctionData::ShiftA { noise } => DensityFunction::ShiftA(ShiftA {
            noise_id: noise.clone(),
            noise: None,
        }),
        DensityFunctionData::ShiftB { noise } => DensityFunction::ShiftB(ShiftB {
            noise_id: noise.clone(),
            noise: None,
        }),
        DensityFunctionData::Shift { noise } => DensityFunction::Shift(Shift {
            noise_id: noise.clone(),
            noise: None,
        }),

        DensityFunctionData::Clamp { input, min, max } => DensityFunction::Clamp(Clamp {
            input: Arc::new(json_to_df(input)),
            min: *min,
            max: *max,
        }),

        DensityFunctionData::Abs { input } => json_mapped(MappedType::Abs, input),
        DensityFunctionData::Square { input } => json_mapped(MappedType::Square, input),
        DensityFunctionData::Cube { input } => json_mapped(MappedType::Cube, input),
        DensityFunctionData::HalfNegative { input } => json_mapped(MappedType::HalfNegative, input),
        DensityFunctionData::QuarterNegative { input } => {
            json_mapped(MappedType::QuarterNegative, input)
        }
        DensityFunctionData::Invert { input } => json_mapped(MappedType::Invert, input),
        DensityFunctionData::Squeeze { input } => json_mapped(MappedType::Squeeze, input),

        DensityFunctionData::Add {
            argument1,
            argument2,
        } => json_two_arg(TwoArgType::Add, argument1, argument2),
        DensityFunctionData::Mul {
            argument1,
            argument2,
        } => json_two_arg(TwoArgType::Mul, argument1, argument2),
        DensityFunctionData::Min {
            argument1,
            argument2,
        } => json_two_arg(TwoArgType::Min, argument1, argument2),
        DensityFunctionData::Max {
            argument1,
            argument2,
        } => json_two_arg(TwoArgType::Max, argument1, argument2),

        DensityFunctionData::Spline { spline } => DensityFunction::Spline(Spline {
            spline: Arc::new(json_spline_to_cubic(spline)),
        }),

        DensityFunctionData::RangeChoice {
            input,
            min_inclusive,
            max_exclusive,
            when_in_range,
            when_out_of_range,
        } => DensityFunction::RangeChoice(RangeChoice {
            input: Arc::new(json_to_df(input)),
            min_inclusive: *min_inclusive,
            max_exclusive: *max_exclusive,
            when_in_range: Arc::new(json_to_df(when_in_range)),
            when_out_of_range: Arc::new(json_to_df(when_out_of_range)),
        }),

        DensityFunctionData::Interpolated { argument } => {
            json_marker(MarkerType::Interpolated, argument)
        }
        DensityFunctionData::FlatCache { argument } => json_marker(MarkerType::FlatCache, argument),
        DensityFunctionData::CacheOnce { argument } => json_marker(MarkerType::CacheOnce, argument),
        DensityFunctionData::Cache2d { argument } => json_marker(MarkerType::Cache2D, argument),
        DensityFunctionData::CacheAllInCell { argument } => {
            json_marker(MarkerType::CacheAllInCell, argument)
        }

        DensityFunctionData::BlendOffset {} => DensityFunction::BlendOffset(BlendOffset),
        DensityFunctionData::BlendAlpha {} => DensityFunction::BlendAlpha(BlendAlpha),
        DensityFunctionData::BlendDensity { input } => {
            DensityFunction::BlendDensity(BlendDensity {
                input: Arc::new(json_to_df(input)),
            })
        }

        // TODO: Implement Beardifier for structure terrain adaptation.
        // Constant(0.0) is correct when structures are not yet generated.
        DensityFunctionData::Beardifier {} => DensityFunction::Constant(Constant { value: 0.0 }),
        DensityFunctionData::EndIslands {} => DensityFunction::EndIslands(EndIslands),

        DensityFunctionData::WeirdScaledSampler {
            input,
            noise,
            rarity_value_mapper,
        } => {
            let mapper = match rarity_value_mapper.as_str() {
                "type_1" => RarityValueMapper::Tunnels,
                _ => RarityValueMapper::Caves,
            };
            DensityFunction::WeirdScaledSampler(WeirdScaledSampler {
                input: Arc::new(json_to_df(input)),
                noise_id: noise.clone(),
                rarity_value_mapper: mapper,
                noise: None,
            })
        }

        DensityFunctionData::OldBlendedNoise {
            xz_scale,
            y_scale,
            xz_factor,
            y_factor,
            smear_scale_multiplier,
        } => DensityFunction::BlendedNoise(BlendedNoise {
            xz_scale: *xz_scale,
            y_scale: *y_scale,
            xz_factor: *xz_factor,
            y_factor: *y_factor,
            smear_scale_multiplier: *smear_scale_multiplier,
            noise: None,
        }),

        DensityFunctionData::FindTopSurface {} => {
            // find_top_surface is unused; treat as constant 0
            DensityFunction::Constant(Constant { value: 0.0 })
        }
    }
}

fn json_mapped(op: MappedType, input: &DensityFunctionJson) -> DensityFunction {
    DensityFunction::Mapped(Mapped {
        op,
        input: Arc::new(json_to_df(input)),
    })
}

fn json_two_arg(
    op: TwoArgType,
    a: &DensityFunctionJson,
    b: &DensityFunctionJson,
) -> DensityFunction {
    DensityFunction::TwoArgumentSimple(TwoArgumentSimple {
        op,
        argument1: Arc::new(json_to_df(a)),
        argument2: Arc::new(json_to_df(b)),
    })
}

fn json_marker(kind: MarkerType, argument: &DensityFunctionJson) -> DensityFunction {
    DensityFunction::Marker(Marker {
        kind,
        wrapped: Arc::new(json_to_df(argument)),
    })
}

fn json_spline_to_cubic(spline: &SplineJson) -> CubicSpline {
    match spline {
        SplineJson::Constant(v) => CubicSpline {
            coordinate: Arc::new(DensityFunction::constant(0.0)),
            points: vec![SplinePoint {
                location: 0.0,
                value: SplineValue::Constant(*v),
                derivative: 0.0,
            }],
        },
        SplineJson::Multipoint { coordinate, points } => CubicSpline {
            coordinate: Arc::new(DensityFunction::Reference(Reference {
                id: coordinate.clone(),
                resolved: None,
            })),
            points: points.iter().map(json_spline_point).collect(),
        },
    }
}

fn json_spline_point(p: &SplinePointJson) -> SplinePoint {
    SplinePoint {
        location: p.location,
        value: match &p.value {
            SplineJson::Constant(v) => SplineValue::Constant(*v),
            multi @ SplineJson::Multipoint { .. } => {
                SplineValue::Spline(Arc::new(json_spline_to_cubic(multi)))
            }
        },
        derivative: p.derivative,
    }
}

// ── Build entry point ───────────────────────────────────────────────────────

use steel_utils::density::transpiler::{TranspilerInput, transpile};

/// Generate the complete density functions module using the transpiler.
pub(crate) fn build() -> TokenStream {
    let router_json = read_overworld_noise_router();
    let registry_json = read_density_function_registry();

    // Convert JSON registry to DensityFunction values
    let registry: BTreeMap<String, DensityFunction> = registry_json
        .iter()
        .map(|(id, json)| (id.clone(), json_to_df(json)))
        .collect();

    // Convert router entries to DensityFunction values
    let mut router_entries = BTreeMap::new();
    router_entries.insert("barrier".to_string(), json_to_df(&router_json.barrier));
    router_entries.insert(
        "fluid_level_floodedness".to_string(),
        json_to_df(&router_json.fluid_level_floodedness),
    );
    router_entries.insert(
        "fluid_level_spread".to_string(),
        json_to_df(&router_json.fluid_level_spread),
    );
    router_entries.insert("lava".to_string(), json_to_df(&router_json.lava));
    router_entries.insert(
        "temperature".to_string(),
        json_to_df(&router_json.temperature),
    );
    router_entries.insert(
        "vegetation".to_string(),
        json_to_df(&router_json.vegetation),
    );
    router_entries.insert(
        "continentalness".to_string(),
        json_to_df(&router_json.continents),
    );
    router_entries.insert("erosion".to_string(), json_to_df(&router_json.erosion));
    router_entries.insert("depth".to_string(), json_to_df(&router_json.depth));
    router_entries.insert("ridges".to_string(), json_to_df(&router_json.ridges));
    router_entries.insert(
        "final_density".to_string(),
        json_to_df(&router_json.final_density),
    );
    router_entries.insert(
        "vein_toggle".to_string(),
        json_to_df(&router_json.vein_toggle),
    );
    router_entries.insert(
        "vein_ridged".to_string(),
        json_to_df(&router_json.vein_ridged),
    );
    router_entries.insert("vein_gap".to_string(), json_to_df(&router_json.vein_gap));
    if let Some(ref psl) = router_json.preliminary_surface_level {
        router_entries.insert("preliminary_surface_level".to_string(), json_to_df(psl));
    }

    let input = TranspilerInput {
        registry,
        router_entries,
    };

    transpile(&input)
}
