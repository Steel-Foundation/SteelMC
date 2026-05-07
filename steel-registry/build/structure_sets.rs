use std::fs;

use rustc_hash::FxHashMap as HashMap;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct StructureSetJson {
    structures: Vec<StructureEntryJson>,
    placement: PlacementJson,
}

#[derive(Deserialize, Debug)]
struct StructureEntryJson {
    structure: String,
    weight: i32,
}

#[derive(Deserialize, Debug)]
struct PlacementJson {
    #[serde(rename = "type")]
    placement_type: String,
    salt: i32,
    #[serde(default = "default_frequency")]
    frequency: f32,
    #[serde(default)]
    frequency_reduction_method: Option<String>,
    #[serde(default)]
    exclusion_zone: Option<ExclusionZoneJson>,
    // RandomSpread fields
    #[serde(default)]
    spacing: Option<i32>,
    #[serde(default)]
    separation: Option<i32>,
    #[serde(default)]
    spread_type: Option<String>,
    // ConcentricRings fields
    #[serde(default)]
    distance: Option<i32>,
    #[serde(default)]
    spread: Option<i32>,
    #[serde(default)]
    count: Option<i32>,
    #[serde(default)]
    preferred_biomes: Option<String>,
}

fn default_frequency() -> f32 {
    1.0
}

#[derive(Deserialize, Debug)]
struct ExclusionZoneJson {
    other_set: String,
    chunk_count: i32,
}

/// Structure JSON — we need biomes, type, and height config.
#[derive(Deserialize, Debug)]
struct StructureJson {
    biomes: String,
    #[serde(rename = "type")]
    structure_type: String,
    #[serde(default)]
    spawn_overrides: HashMap<String, SpawnOverrideJson>,
    step: String,
    #[serde(default)]
    terrain_adaptation: Option<String>,
    #[serde(default)]
    start_height: Option<serde_json::Value>,
    #[serde(default)]
    project_start_to_heightmap: Option<String>,
    // Jigsaw-specific fields
    #[serde(default)]
    start_pool: Option<String>,
    #[serde(default, rename = "size")]
    max_depth: Option<i32>,
    #[serde(default)]
    use_expansion_hack: Option<bool>,
    #[serde(default)]
    max_distance_from_center: Option<serde_json::Value>,
    #[serde(default)]
    start_jigsaw_name: Option<String>,
    #[serde(default)]
    dimension_padding: Option<serde_json::Value>,
    #[serde(default)]
    pool_aliases: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    liquid_settings: Option<String>,
    #[serde(default)]
    mineshaft_type: Option<String>,
    #[serde(default)]
    is_beached: Option<bool>,
    #[serde(default)]
    biome_temp: Option<String>,
    #[serde(default)]
    large_probability: Option<f32>,
    #[serde(default)]
    cluster_probability: Option<f32>,
    #[serde(default)]
    setups: Option<Vec<RuinedPortalSetupJson>>,
    #[serde(default)]
    height: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
struct SpawnOverrideJson {
    bounding_box: String,
    spawns: Vec<SpawnerJson>,
}

#[derive(Deserialize, Debug)]
struct SpawnerJson {
    #[serde(rename = "type")]
    entity_type: String,
    weight: i32,
    #[serde(rename = "minCount")]
    min_count: i32,
    #[serde(rename = "maxCount")]
    max_count: i32,
}

#[derive(Deserialize, Debug)]
struct RuinedPortalSetupJson {
    placement: String,
    weight: f32,
    air_pocket_probability: f32,
    can_be_cold: bool,
    mossiness: f32,
    overgrown: bool,
    replace_with_blackstone: bool,
    vines: bool,
}

/// Biome tag JSON.
#[derive(Deserialize, Debug)]
struct TagJson {
    values: Vec<String>,
}

/// Loads all biome tags from the worldgen/biome tags directory,
/// then recursively resolves tag references to flat biome lists.
fn load_biome_tags() -> HashMap<String, Vec<String>> {
    let tag_base = "build_assets/builtin_datapacks/minecraft/tags/worldgen/biome";

    // First pass: load raw tag definitions (may contain #tag references)
    let mut raw_tags: HashMap<String, Vec<String>> = HashMap::default();
    load_tags_from_dir(tag_base, "", &mut raw_tags);

    // Second pass: resolve all tag references recursively
    let keys: Vec<String> = raw_tags.keys().cloned().collect();
    let mut resolved: HashMap<String, Vec<String>> = HashMap::default();
    for key in &keys {
        let biomes = resolve_tag(key, &raw_tags, &mut resolved, &mut Vec::new());
        resolved.insert(key.clone(), biomes);
    }

    resolved
}

fn load_tags_from_dir(dir: &str, prefix: &str, tags: &mut HashMap<String, Vec<String>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            let dir_name = path.file_name().unwrap().to_str().unwrap();
            let new_prefix = if prefix.is_empty() {
                dir_name.to_string()
            } else {
                format!("{prefix}/{dir_name}")
            };
            load_tags_from_dir(path.to_str().unwrap(), &new_prefix, tags);
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let tag_name = path.file_stem().unwrap().to_str().unwrap();
            let full_name = if prefix.is_empty() {
                format!("minecraft:{tag_name}")
            } else {
                format!("minecraft:{prefix}/{tag_name}")
            };
            let content = fs::read_to_string(&path).unwrap();
            let tag: TagJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse biome tag {full_name}: {e}"));
            tags.insert(full_name, tag.values);
        }
    }
}

fn resolve_tag(
    tag_name: &str,
    raw_tags: &HashMap<String, Vec<String>>,
    cache: &mut HashMap<String, Vec<String>>,
    stack: &mut Vec<String>,
) -> Vec<String> {
    if let Some(cached) = cache.get(tag_name) {
        return cached.clone();
    }

    // Cycle detection
    if stack.contains(&tag_name.to_string()) {
        panic!("Circular biome tag reference: {stack:?} -> {tag_name}");
    }
    stack.push(tag_name.to_string());

    let Some(values) = raw_tags.get(tag_name) else {
        stack.pop();
        return vec![];
    };

    let mut result = Vec::new();
    for value in values {
        if let Some(referenced_tag) = value.strip_prefix('#') {
            // Recursive tag reference
            let resolved = resolve_tag(referenced_tag, raw_tags, cache, stack);
            result.extend(resolved);
        } else {
            // Direct biome identifier
            result.push(value.clone());
        }
    }

    result.sort();
    result.dedup();
    stack.pop();
    cache.insert(tag_name.to_string(), result.clone());
    result
}

/// Per-structure data extracted from the structure JSON.
struct StructureData {
    allowed_biomes: Vec<String>,
    structure_type: String,
    spawn_overrides: Vec<SpawnOverrideData>,
    step: String,
    terrain_adaptation: Option<String>,
    config: StructureConfigData,
}

struct SpawnOverrideData {
    category: String,
    bounding_box: String,
    spawns: Vec<SpawnerJson>,
}

enum StructureConfigData {
    Jigsaw(JigsawConfigData),
    Mineshaft {
        mineshaft_type: String,
    },
    Shipwreck {
        is_beached: bool,
    },
    OceanRuin {
        biome_temp: String,
        large_probability: f32,
        cluster_probability: f32,
    },
    RuinedPortal {
        setups: Vec<RuinedPortalSetupJson>,
    },
    NetherFossil {
        height: HeightProviderData,
    },
    Empty,
}

/// Build-time representation of jigsaw config.
struct JigsawConfigData {
    start_pool: String,
    max_depth: i32,
    use_expansion_hack: bool,
    project_start_to_heightmap: Option<String>,
    start_height: StartHeightData,
    max_distance_from_center: i32,
    start_jigsaw_name: Option<String>,
    dimension_padding: (i32, i32),
    pool_aliases: Vec<serde_json::Value>,
    liquid_settings: Option<String>,
}

enum StartHeightData {
    Constant(i32),
    Uniform { min: i32, max: i32 },
}

enum VerticalAnchorData {
    Absolute(i32),
    AboveBottom(i32),
    BelowTop(i32),
}

enum HeightProviderData {
    Constant(VerticalAnchorData),
    Uniform {
        min_inclusive: VerticalAnchorData,
        max_inclusive: VerticalAnchorData,
    },
}

fn parse_start_height_full(value: &serde_json::Value) -> StartHeightData {
    // {"absolute": N}
    if let Some(n) = value.get("absolute").and_then(|v| v.as_i64()) {
        return StartHeightData::Constant(n as i32);
    }
    // {"type": "minecraft:uniform", ...}
    if value.get("type").and_then(|v| v.as_str()) == Some("minecraft:uniform") {
        let min = value
            .get("min_inclusive")
            .and_then(|v| v.get("absolute"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let max = value
            .get("max_inclusive")
            .and_then(|v| v.get("absolute"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        return StartHeightData::Uniform { min, max };
    }
    // {"type": "minecraft:constant", "value": ...}
    if let Some(v) = value.get("value") {
        if let Some(n) = v.as_i64() {
            return StartHeightData::Constant(n as i32);
        }
        if let Some(n) = v.get("absolute").and_then(|v| v.as_i64()) {
            return StartHeightData::Constant(n as i32);
        }
    }
    StartHeightData::Constant(0)
}

fn parse_vertical_anchor(value: &serde_json::Value) -> VerticalAnchorData {
    if let Some(n) = value.get("absolute").and_then(|v| v.as_i64()) {
        return VerticalAnchorData::Absolute(n as i32);
    }
    if let Some(n) = value.get("above_bottom").and_then(|v| v.as_i64()) {
        return VerticalAnchorData::AboveBottom(n as i32);
    }
    if let Some(n) = value.get("below_top").and_then(|v| v.as_i64()) {
        return VerticalAnchorData::BelowTop(n as i32);
    }
    VerticalAnchorData::Absolute(0)
}

fn parse_height_provider(value: &serde_json::Value) -> HeightProviderData {
    if value.get("type").and_then(|v| v.as_str()) == Some("minecraft:uniform") {
        let min = value
            .get("min_inclusive")
            .map(parse_vertical_anchor)
            .unwrap_or(VerticalAnchorData::Absolute(0));
        let max = value
            .get("max_inclusive")
            .map(parse_vertical_anchor)
            .unwrap_or(VerticalAnchorData::Absolute(0));
        return HeightProviderData::Uniform {
            min_inclusive: min,
            max_inclusive: max,
        };
    }

    if let Some(v) = value.get("value") {
        return HeightProviderData::Constant(parse_vertical_anchor(v));
    }
    HeightProviderData::Constant(parse_vertical_anchor(value))
}

/// Loads structure definitions and resolves biome tags for each structure.
fn load_structure_data(
    biome_tags: &HashMap<String, Vec<String>>,
) -> HashMap<String, StructureData> {
    let structure_dir = "build_assets/builtin_datapacks/minecraft/worldgen/structure";
    let mut result = HashMap::default();

    for entry in fs::read_dir(structure_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap();
        let full_name = format!("minecraft:{name}");
        let content = fs::read_to_string(&path).unwrap();
        let structure: StructureJson = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse structure {full_name}: {e}"));

        let allowed_biomes = if let Some(tag_name) = structure.biomes.strip_prefix('#') {
            biome_tags.get(tag_name).cloned().unwrap_or_default()
        } else {
            vec![structure.biomes.clone()]
        };

        let spawn_overrides = structure
            .spawn_overrides
            .into_iter()
            .map(|(category, override_data)| SpawnOverrideData {
                category,
                bounding_box: override_data.bounding_box,
                spawns: override_data.spawns,
            })
            .collect();

        let config = match structure.structure_type.as_str() {
            "minecraft:jigsaw" => {
                let start_pool = structure.start_pool.clone().unwrap_or_default();
                let max_depth = structure.max_depth.unwrap_or(0);
                let use_expansion_hack = structure.use_expansion_hack.unwrap_or(false);
                let start_height = structure
                    .start_height
                    .as_ref()
                    .map(parse_start_height_full)
                    .unwrap_or(StartHeightData::Constant(0));
                let max_distance_from_center = structure
                    .max_distance_from_center
                    .as_ref()
                    .and_then(|v| v.as_i64())
                    .unwrap_or(80) as i32;
                let dim_pad = structure
                    .dimension_padding
                    .as_ref()
                    .and_then(|v| v.as_i64().map(|n| (n as i32, n as i32)))
                    .or_else(|| {
                        structure.dimension_padding.as_ref().map(|v| {
                            let bottom =
                                v.get("bottom").and_then(|b| b.as_i64()).unwrap_or(0) as i32;
                            let top = v.get("top").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                            (bottom, top)
                        })
                    })
                    .unwrap_or((0, 0));

                StructureConfigData::Jigsaw(JigsawConfigData {
                    start_pool,
                    max_depth,
                    use_expansion_hack,
                    project_start_to_heightmap: structure.project_start_to_heightmap.clone(),
                    start_height,
                    max_distance_from_center,
                    start_jigsaw_name: structure.start_jigsaw_name.clone(),
                    dimension_padding: dim_pad,
                    pool_aliases: structure.pool_aliases.clone().unwrap_or_default(),
                    liquid_settings: structure.liquid_settings.clone(),
                })
            }
            "minecraft:mineshaft" => StructureConfigData::Mineshaft {
                mineshaft_type: structure
                    .mineshaft_type
                    .clone()
                    .unwrap_or_else(|| "normal".to_string()),
            },
            "minecraft:shipwreck" => StructureConfigData::Shipwreck {
                is_beached: structure.is_beached.unwrap_or(false),
            },
            "minecraft:ocean_ruin" => StructureConfigData::OceanRuin {
                biome_temp: structure
                    .biome_temp
                    .clone()
                    .unwrap_or_else(|| "cold".to_string()),
                large_probability: structure.large_probability.unwrap_or(0.3),
                cluster_probability: structure.cluster_probability.unwrap_or(0.9),
            },
            "minecraft:ruined_portal" => StructureConfigData::RuinedPortal {
                setups: structure.setups.unwrap_or_default(),
            },
            "minecraft:nether_fossil" => StructureConfigData::NetherFossil {
                height: structure
                    .height
                    .as_ref()
                    .map(parse_height_provider)
                    .unwrap_or(HeightProviderData::Constant(VerticalAnchorData::Absolute(
                        0,
                    ))),
            },
            _ => StructureConfigData::Empty,
        };

        result.insert(
            full_name,
            StructureData {
                allowed_biomes,
                structure_type: structure.structure_type.clone(),
                spawn_overrides,
                step: structure.step,
                terrain_adaptation: structure.terrain_adaptation,
                config,
            },
        );
    }

    result
}

fn generate_frequency_method(method: &Option<String>) -> TokenStream {
    match method.as_deref() {
        None | Some("default") => quote! { FrequencyMethodData::Default },
        Some("legacy_type_1") => quote! { FrequencyMethodData::LegacyType1 },
        Some("legacy_type_2") => quote! { FrequencyMethodData::LegacyType2 },
        Some("legacy_type_3") => quote! { FrequencyMethodData::LegacyType3 },
        Some(other) => panic!("Unknown frequency reduction method: {other}"),
    }
}

fn generate_spread_type(spread: &Option<String>) -> TokenStream {
    match spread.as_deref() {
        None | Some("linear") => quote! { SpreadTypeData::Linear },
        Some("triangular") => quote! { SpreadTypeData::Triangular },
        Some(other) => panic!("Unknown spread type: {other}"),
    }
}

fn generate_identifier(id: &str) -> TokenStream {
    if let Some((namespace, path)) = id.split_once(':') {
        quote! { Identifier::new(#namespace, #path) }
    } else {
        quote! { Identifier::vanilla(#id.to_string()) }
    }
}

fn structure_static_ident(key: &str) -> proc_macro2::Ident {
    let name = key
        .strip_prefix("minecraft:")
        .unwrap_or(key)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    format_ident!("STRUCTURE_{name}")
}

fn generate_generation_step(step: &str) -> TokenStream {
    match step {
        "surface_structures" => quote! { StructureGenerationStep::SurfaceStructures },
        "underground_structures" => quote! { StructureGenerationStep::UndergroundStructures },
        "underground_decoration" => quote! { StructureGenerationStep::UndergroundDecoration },
        other => panic!("Unknown structure generation step: {other}"),
    }
}

fn generate_terrain_adjustment(adjustment: Option<&str>) -> TokenStream {
    match adjustment.unwrap_or("none") {
        "none" => quote! { TerrainAdjustment::None },
        "bury" => quote! { TerrainAdjustment::Bury },
        "beard_thin" => quote! { TerrainAdjustment::BeardThin },
        "beard_box" => quote! { TerrainAdjustment::BeardBox },
        "encapsulate" => quote! { TerrainAdjustment::Encapsulate },
        other => panic!("Unknown terrain adjustment: {other}"),
    }
}

fn generate_spawn_bounding_box(bounding_box: &str) -> TokenStream {
    match bounding_box {
        "full" => quote! { StructureSpawnBoundingBox::Full },
        "piece" => quote! { StructureSpawnBoundingBox::Piece },
        other => panic!("Unknown structure spawn override bounding box: {other}"),
    }
}

fn generate_spawn_overrides(overrides: &[SpawnOverrideData]) -> Vec<TokenStream> {
    overrides
        .iter()
        .map(|override_data| {
            let category = override_data.category.as_str();
            let bounding_box = generate_spawn_bounding_box(&override_data.bounding_box);
            let spawns: Vec<TokenStream> = override_data
                .spawns
                .iter()
                .map(|spawn| {
                    let entity_type = generate_identifier(&spawn.entity_type);
                    let weight = spawn.weight;
                    let min_count = spawn.min_count;
                    let max_count = spawn.max_count;
                    quote! {
                        StructureSpawnerData {
                            entity_type: #entity_type,
                            weight: #weight,
                            min_count: #min_count,
                            max_count: #max_count,
                        }
                    }
                })
                .collect();
            quote! {
                StructureSpawnOverrideData {
                    category: #category.to_string(),
                    bounding_box: #bounding_box,
                    spawns: vec![#(#spawns),*],
                }
            }
        })
        .collect()
}

fn generate_start_height(height: &StartHeightData) -> TokenStream {
    match height {
        StartHeightData::Constant(y) => quote! { StartHeight::Constant(#y) },
        StartHeightData::Uniform { min, max } => {
            quote! { StartHeight::Uniform { min: #min, max: #max } }
        }
    }
}

fn generate_vertical_anchor(anchor: &VerticalAnchorData) -> TokenStream {
    match anchor {
        VerticalAnchorData::Absolute(y) => quote! { VerticalAnchorData::Absolute(#y) },
        VerticalAnchorData::AboveBottom(y) => quote! { VerticalAnchorData::AboveBottom(#y) },
        VerticalAnchorData::BelowTop(y) => quote! { VerticalAnchorData::BelowTop(#y) },
    }
}

fn generate_height_provider(height: &HeightProviderData) -> TokenStream {
    match height {
        HeightProviderData::Constant(anchor) => {
            let anchor = generate_vertical_anchor(anchor);
            quote! { HeightProviderData::Constant(#anchor) }
        }
        HeightProviderData::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min = generate_vertical_anchor(min_inclusive);
            let max = generate_vertical_anchor(max_inclusive);
            quote! { HeightProviderData::Uniform { min_inclusive: #min, max_inclusive: #max } }
        }
    }
}

fn generate_pool_aliases(aliases: &[serde_json::Value]) -> Vec<TokenStream> {
    aliases
        .iter()
        .map(|alias| {
            let alias_type = alias.get("type").and_then(|t| t.as_str()).unwrap_or("");
            match alias_type {
                "minecraft:direct" => {
                    let a = generate_identifier(
                        alias.get("alias").and_then(|v| v.as_str()).unwrap_or(""),
                    );
                    let t = generate_identifier(
                        alias.get("target").and_then(|v| v.as_str()).unwrap_or(""),
                    );
                    quote! { PoolAlias::Direct { alias: #a, target: #t } }
                }
                "minecraft:random" => {
                    let a = generate_identifier(
                        alias.get("alias").and_then(|v| v.as_str()).unwrap_or(""),
                    );
                    let targets: Vec<TokenStream> = alias
                        .get("targets")
                        .and_then(|t| t.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|t| {
                                    let data = generate_identifier(
                                        t.get("data").and_then(|d| d.as_str()).unwrap_or(""),
                                    );
                                    let w = t.get("weight").and_then(|w| w.as_i64()).unwrap_or(1)
                                        as i32;
                                    quote! { (#data, #w) }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    quote! { PoolAlias::Random { alias: #a, targets: vec![#(#targets),*] } }
                }
                "minecraft:random_group" => {
                    let groups: Vec<TokenStream> = alias
                        .get("groups")
                        .and_then(|g| g.as_array())
                        .map(|arr| {
                            arr.iter()
                                .map(|group| {
                                    let w =
                                        group.get("weight").and_then(|w| w.as_i64()).unwrap_or(1)
                                            as i32;
                                    let bindings: Vec<TokenStream> = group
                                        .get("data")
                                        .and_then(|d| d.as_array())
                                        .map(|data| {
                                            data.iter()
                                                .map(|b| {
                                                    let a = generate_identifier(
                                                        b.get("alias")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or(""),
                                                    );
                                                    let t = generate_identifier(
                                                        b.get("target")
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or(""),
                                                    );
                                                    quote! { (#a, #t) }
                                                })
                                                .collect()
                                        })
                                        .unwrap_or_default();
                                    quote! { (vec![#(#bindings),*], #w) }
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    quote! { PoolAlias::RandomGroup { groups: vec![#(#groups),*] } }
                }
                _ => quote! {},
            }
        })
        .collect()
}

fn generate_liquid_settings(settings: Option<&str>) -> TokenStream {
    match settings.unwrap_or("apply_waterlogging") {
        "apply_waterlogging" => quote! { LiquidSettingsData::ApplyWaterlogging },
        "ignore_waterlogging" => quote! { LiquidSettingsData::IgnoreWaterlogging },
        other => panic!("Unknown jigsaw liquid setting: {other}"),
    }
}

fn generate_jigsaw_config(config: &JigsawConfigData) -> TokenStream {
    let start_pool = generate_identifier(&config.start_pool);
    let max_depth = config.max_depth;
    let use_expansion_hack = config.use_expansion_hack;
    let heightmap_token = match &config.project_start_to_heightmap {
        Some(h) => quote! { Some(#h.to_string()) },
        None => quote! { None },
    };
    let start_height = generate_start_height(&config.start_height);
    let max_distance_from_center = config.max_distance_from_center;
    let start_jigsaw_name = match &config.start_jigsaw_name {
        Some(name) => {
            let id = generate_identifier(name);
            quote! { Some(#id) }
        }
        None => quote! { None },
    };
    let pad_bottom = config.dimension_padding.0;
    let pad_top = config.dimension_padding.1;
    let aliases = generate_pool_aliases(&config.pool_aliases);
    let liquid_settings = generate_liquid_settings(config.liquid_settings.as_deref());

    quote! {
        JigsawConfig {
            start_pool: #start_pool,
            max_depth: #max_depth,
            use_expansion_hack: #use_expansion_hack,
            project_start_to_heightmap: #heightmap_token,
            start_height: #start_height,
            max_distance_from_center: #max_distance_from_center,
            start_jigsaw_name: #start_jigsaw_name,
            dimension_padding: DimensionPadding { bottom: #pad_bottom, top: #pad_top },
            pool_aliases: vec![#(#aliases),*],
            liquid_settings: #liquid_settings,
        }
    }
}

fn generate_mineshaft_type(mineshaft_type: &str) -> TokenStream {
    match mineshaft_type {
        "normal" => quote! { MineshaftTypeData::Normal },
        "mesa" => quote! { MineshaftTypeData::Mesa },
        other => panic!("Unknown mineshaft type: {other}"),
    }
}

fn generate_ocean_ruin_temp(temp: &str) -> TokenStream {
    match temp {
        "warm" => quote! { OceanRuinBiomeTempData::Warm },
        "cold" => quote! { OceanRuinBiomeTempData::Cold },
        other => panic!("Unknown ocean ruin biome temp: {other}"),
    }
}

fn generate_ruined_portal_placement(placement: &str) -> TokenStream {
    match placement {
        "on_land_surface" => quote! { RuinedPortalPlacementData::OnLandSurface },
        "partly_buried" => quote! { RuinedPortalPlacementData::PartlyBuried },
        "underground" => quote! { RuinedPortalPlacementData::Underground },
        "in_mountain" => quote! { RuinedPortalPlacementData::InMountain },
        "on_ocean_floor" => quote! { RuinedPortalPlacementData::OnOceanFloor },
        "in_nether" => quote! { RuinedPortalPlacementData::InNether },
        other => panic!("Unknown ruined portal placement: {other}"),
    }
}

fn generate_structure_config(config: &StructureConfigData) -> TokenStream {
    match config {
        StructureConfigData::Jigsaw(config) => {
            let config = generate_jigsaw_config(config);
            quote! { StructureConfigData::Jigsaw(#config) }
        }
        StructureConfigData::Mineshaft { mineshaft_type } => {
            let mineshaft_type = generate_mineshaft_type(mineshaft_type);
            quote! { StructureConfigData::Mineshaft { mineshaft_type: #mineshaft_type } }
        }
        StructureConfigData::Shipwreck { is_beached } => {
            quote! { StructureConfigData::Shipwreck { is_beached: #is_beached } }
        }
        StructureConfigData::OceanRuin {
            biome_temp,
            large_probability,
            cluster_probability,
        } => {
            let biome_temp = generate_ocean_ruin_temp(biome_temp);
            quote! {
                StructureConfigData::OceanRuin {
                    biome_temp: #biome_temp,
                    large_probability: #large_probability,
                    cluster_probability: #cluster_probability,
                }
            }
        }
        StructureConfigData::RuinedPortal { setups } => {
            let setups: Vec<TokenStream> = setups
                .iter()
                .map(|setup| {
                    let placement = generate_ruined_portal_placement(&setup.placement);
                    let weight = setup.weight;
                    let air_pocket_probability = setup.air_pocket_probability;
                    let can_be_cold = setup.can_be_cold;
                    let mossiness = setup.mossiness;
                    let overgrown = setup.overgrown;
                    let replace_with_blackstone = setup.replace_with_blackstone;
                    let vines = setup.vines;
                    quote! {
                        RuinedPortalSetupData {
                            placement: #placement,
                            weight: #weight,
                            air_pocket_probability: #air_pocket_probability,
                            can_be_cold: #can_be_cold,
                            mossiness: #mossiness,
                            overgrown: #overgrown,
                            replace_with_blackstone: #replace_with_blackstone,
                            vines: #vines,
                        }
                    }
                })
                .collect();
            quote! { StructureConfigData::RuinedPortal { setups: vec![#(#setups),*] } }
        }
        StructureConfigData::NetherFossil { height } => {
            let height = generate_height_provider(height);
            quote! { StructureConfigData::NetherFossil { height: #height } }
        }
        StructureConfigData::Empty => quote! { StructureConfigData::Empty },
    }
}

pub(crate) fn build_structures() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/worldgen/structure/");
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/tags/worldgen/biome/"
    );

    let biome_tags = load_biome_tags();
    let mut structures: Vec<_> = load_structure_data(&biome_tags).into_iter().collect();
    structures.sort_by(|a, b| a.0.cmp(&b.0));

    let mut statics = Vec::new();
    let mut static_refs = Vec::new();

    for (key, structure) in structures {
        let static_ident = structure_static_ident(&key);
        let key_token = generate_identifier(&key);
        let structure_type = generate_identifier(&structure.structure_type);
        let biomes: Vec<TokenStream> = structure
            .allowed_biomes
            .iter()
            .map(|b| generate_identifier(b))
            .collect();
        let spawn_overrides = generate_spawn_overrides(&structure.spawn_overrides);
        let step = generate_generation_step(&structure.step);
        let terrain_adjustment =
            generate_terrain_adjustment(structure.terrain_adaptation.as_deref());
        let config = generate_structure_config(&structure.config);

        statics.push(quote! {
            static #static_ident: LazyLock<StructureData> = LazyLock::new(|| StructureData {
                key: #key_token,
                id: OnceLock::new(),
                structure_type: #structure_type,
                allowed_biomes: vec![#(#biomes),*],
                spawn_overrides: vec![#(#spawn_overrides),*],
                step: #step,
                terrain_adjustment: #terrain_adjustment,
                config: #config,
            });
        });
        static_refs.push(quote! { &*#static_ident });
    }

    quote! {
        use crate::structure::{
            DimensionPadding, HeightProviderData, JigsawConfig, LiquidSettingsData,
            MineshaftTypeData, OceanRuinBiomeTempData, PoolAlias, RuinedPortalPlacementData,
            RuinedPortalSetupData, StartHeight, StructureConfigData, StructureData,
            StructureGenerationStep, StructureRef, StructureRegistry, StructureSpawnBoundingBox,
            StructureSpawnOverrideData, StructureSpawnerData, TerrainAdjustment, VerticalAnchorData,
        };
        use steel_utils::Identifier;
        use std::sync::{LazyLock, OnceLock};

        #(#statics)*

        pub fn register_structures(registry: &mut StructureRegistry) {
            #(registry.register(#static_refs);)*
        }

        pub fn vanilla_structures() -> Vec<StructureRef> {
            vec![#(#static_refs),*]
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/worldgen/structure_set/"
    );
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/tags/worldgen/biome/"
    );

    // Load and resolve biome tags for concentric-ring preferred biome tags.
    let biome_tags = load_biome_tags();

    let set_dir = "build_assets/builtin_datapacks/minecraft/worldgen/structure_set";
    let mut sets = Vec::new();

    for entry in fs::read_dir(set_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let set_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let set: StructureSetJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse structure set {set_name}: {e}"));
            sets.push((set_name, set));
        }
    }

    // Sort for deterministic output
    sets.sort_by(|a, b| a.0.cmp(&b.0));

    let mut entries = TokenStream::new();

    for (set_name, set) in &sets {
        let key = generate_identifier(&format!("minecraft:{set_name}"));

        let structures: Vec<TokenStream> = set
            .structures
            .iter()
            .map(|entry| {
                let structure = generate_identifier(&entry.structure);
                let weight = entry.weight;

                quote! {
                    StructureEntryData {
                        structure: #structure,
                        weight: #weight,
                    }
                }
            })
            .collect();

        let freq = set.placement.frequency;
        let freq_method = generate_frequency_method(&set.placement.frequency_reduction_method);

        let placement = match set.placement.placement_type.as_str() {
            "minecraft:random_spread" => {
                let spacing = set.placement.spacing.unwrap();
                let separation = set.placement.separation.unwrap();
                let salt = set.placement.salt;
                let spread_type = generate_spread_type(&set.placement.spread_type);

                let exclusion = if let Some(ez) = &set.placement.exclusion_zone {
                    let other = generate_identifier(&ez.other_set);
                    let count = ez.chunk_count;
                    quote! {
                        Some(ExclusionZoneData {
                            other_set: #other,
                            chunk_count: #count,
                        })
                    }
                } else {
                    quote! { None }
                };

                quote! {
                    PlacementData::RandomSpread {
                        spacing: #spacing,
                        separation: #separation,
                        spread_type: #spread_type,
                        salt: #salt,
                        frequency: #freq,
                        frequency_reduction_method: #freq_method,
                        exclusion_zone: #exclusion,
                    }
                }
            }
            "minecraft:concentric_rings" => {
                let distance = set.placement.distance.unwrap();
                let spread = set.placement.spread.unwrap();
                let count = set.placement.count.unwrap();
                let salt = set.placement.salt;

                // Resolve preferred biomes from tag reference (e.g., "#minecraft:stronghold_biased_to")
                let preferred_biomes: Vec<String> =
                    if let Some(ref tag_ref) = set.placement.preferred_biomes {
                        if let Some(tag_name) = tag_ref.strip_prefix('#') {
                            biome_tags.get(tag_name).cloned().unwrap_or_default()
                        } else {
                            // Direct biome identifier
                            vec![tag_ref.clone()]
                        }
                    } else {
                        vec![]
                    };
                let biome_tokens: Vec<TokenStream> = preferred_biomes
                    .iter()
                    .map(|b| generate_identifier(b))
                    .collect();

                quote! {
                    PlacementData::ConcentricRings {
                        distance: #distance,
                        spread: #spread,
                        count: #count,
                        preferred_biomes: vec![#(#biome_tokens),*],
                        salt: #salt,
                        frequency: #freq,
                        frequency_reduction_method: #freq_method,
                    }
                }
            }
            other => panic!("Unknown placement type: {other}"),
        };

        entries.extend(quote! {
            StructureSetData {
                key: #key,
                structures: vec![#(#structures),*],
                placement: #placement,
            },
        });
    }

    quote! {
        use crate::structure_set::{
            StructureSetData, StructureEntryData, PlacementData,
            SpreadTypeData, FrequencyMethodData, ExclusionZoneData,
        };
        use steel_utils::Identifier;

        /// Returns all vanilla structure sets parsed from the datapack.
        pub fn vanilla_structure_sets() -> Vec<StructureSetData> {
            vec![#entries]
        }
    }
}
