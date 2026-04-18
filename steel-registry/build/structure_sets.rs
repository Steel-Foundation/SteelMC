use std::collections::HashMap;
use std::fs;

use proc_macro2::TokenStream;
use quote::quote;
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
    terrain_adaptation: Option<String>,
    #[serde(default)]
    pool_aliases: Option<Vec<serde_json::Value>>,
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
    let mut raw_tags: HashMap<String, Vec<String>> = HashMap::new();
    load_tags_from_dir(tag_base, "", &mut raw_tags);

    // Second pass: resolve all tag references recursively
    let keys: Vec<String> = raw_tags.keys().cloned().collect();
    let mut resolved: HashMap<String, Vec<String>> = HashMap::new();
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
    biome_check_y: Option<i32>,
    structure_type: String,
    jigsaw_config: Option<JigsawConfigData>,
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
    terrain_adaptation: String,
    pool_aliases: Vec<serde_json::Value>,
}

enum StartHeightData {
    Constant(i32),
    Uniform { min: i32, max: i32 },
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

/// Parses a HeightProvider value to extract a representative Y level.
/// Returns the absolute value for constant providers, or the midpoint for ranges.
fn parse_start_height(value: &serde_json::Value) -> Option<i32> {
    // {"absolute": N}
    if let Some(n) = value.get("absolute").and_then(|v| v.as_i64()) {
        return Some(n as i32);
    }
    // {"type": "minecraft:uniform", "min_inclusive": {"absolute": N}, "max_inclusive": {"absolute": M}}
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
        return Some((min + max) / 2);
    }
    // {"type": "minecraft:constant", "value": N} or {"type": "minecraft:constant", "value": {"absolute": N}}
    if value.get("type").and_then(|v| v.as_str()) == Some("minecraft:constant")
        && let Some(v) = value.get("value") {
            if let Some(n) = v.as_i64() {
                return Some(n as i32);
            }
            if let Some(n) = v.get("absolute").and_then(|v| v.as_i64()) {
                return Some(n as i32);
            }
        }
    None
}

/// Loads structure definitions to get biomes and biome check Y for each structure.
fn load_structure_data(
    biome_tags: &HashMap<String, Vec<String>>,
) -> HashMap<String, StructureData> {
    let structure_dir =
        "build_assets/builtin_datapacks/minecraft/worldgen/structure";
    let mut result = HashMap::new();

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

        // Determine biome check Y based on structure type
        let biome_check_y = match structure.structure_type.as_str() {
            "minecraft:jigsaw" => {
                if structure.project_start_to_heightmap.is_some() {
                    // Projected to heightmap → use surface height
                    None
                } else if let Some(ref height) = structure.start_height {
                    // Fixed/sampled start height → use that Y for biome check
                    parse_start_height(height)
                } else {
                    None
                }
            }
            // Mineshaft checks biome at y=50 (generation center)
            "minecraft:mineshaft" => Some(50),
            // Nether fortress at y=64
            "minecraft:fortress" => Some(64),
            // All other types check at surface height
            _ => None,
        };

        // Extract jigsaw config if this is a jigsaw structure
        let jigsaw_config = if structure.structure_type == "minecraft:jigsaw" {
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
                        let bottom = v.get("bottom").and_then(|b| b.as_i64()).unwrap_or(0) as i32;
                        let top = v.get("top").and_then(|t| t.as_i64()).unwrap_or(0) as i32;
                        (bottom, top)
                    })
                })
                .unwrap_or((0, 0));

            Some(JigsawConfigData {
                start_pool,
                max_depth,
                use_expansion_hack,
                project_start_to_heightmap: structure.project_start_to_heightmap.clone(),
                start_height,
                max_distance_from_center,
                start_jigsaw_name: structure.start_jigsaw_name.clone(),
                dimension_padding: dim_pad,
                terrain_adaptation: structure
                    .terrain_adaptation
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                pool_aliases: structure.pool_aliases.clone().unwrap_or_default(),
            })
        } else {
            None
        };

        result.insert(
            full_name,
            StructureData {
                allowed_biomes,
                biome_check_y,
                structure_type: structure.structure_type.clone(),
                jigsaw_config,
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

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/worldgen/structure_set/"
    );
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/worldgen/structure/"
    );
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/tags/worldgen/biome/"
    );

    // Load and resolve biome tags, then get per-structure data
    let biome_tags = load_biome_tags();
    let structure_data = load_structure_data(&biome_tags);

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

                let data = structure_data.get(&entry.structure);
                let biomes = data
                    .map(|d| d.allowed_biomes.clone())
                    .unwrap_or_default();
                let biome_tokens: Vec<TokenStream> =
                    biomes.iter().map(|b| generate_identifier(b)).collect();

                let biome_check_y = data.and_then(|d| d.biome_check_y);
                let biome_check_y_token = match biome_check_y {
                    Some(y) => quote! { Some(#y) },
                    None => quote! { None },
                };

                let stype = data
                    .map(|d| d.structure_type.as_str())
                    .unwrap_or("minecraft:unknown");

                let jigsaw_config_token = match data.and_then(|d| d.jigsaw_config.as_ref()) {
                    Some(jc) => {
                        let start_pool = generate_identifier(&jc.start_pool);
                        let max_depth = jc.max_depth;
                        let use_expansion_hack = jc.use_expansion_hack;
                        let heightmap_token = match &jc.project_start_to_heightmap {
                            Some(h) => { let h = h.as_str(); quote! { Some(#h.to_string()) } }
                            None => quote! { None },
                        };
                        let start_height_token = match &jc.start_height {
                            StartHeightData::Constant(y) => quote! { StartHeight::Constant(#y) },
                            StartHeightData::Uniform { min, max } => quote! { StartHeight::Uniform { min: #min, max: #max } },
                        };
                        let max_dist = jc.max_distance_from_center;
                        let jigsaw_name_token = match &jc.start_jigsaw_name {
                            Some(n) => { let id = generate_identifier(n); quote! { Some(#id) } }
                            None => quote! { None },
                        };
                        let pad_bottom = jc.dimension_padding.0;
                        let pad_top = jc.dimension_padding.1;
                        let terrain = &jc.terrain_adaptation;

                        let alias_tokens: Vec<TokenStream> = jc.pool_aliases.iter().map(|alias| {
                            let alias_type = alias.get("type").and_then(|t| t.as_str()).unwrap_or("");
                            match alias_type {
                                "minecraft:direct" => {
                                    let a = generate_identifier(alias.get("alias").and_then(|v| v.as_str()).unwrap_or(""));
                                    let t = generate_identifier(alias.get("target").and_then(|v| v.as_str()).unwrap_or(""));
                                    quote! { PoolAlias::Direct { alias: #a, target: #t } }
                                }
                                "minecraft:random" => {
                                    let a = generate_identifier(alias.get("alias").and_then(|v| v.as_str()).unwrap_or(""));
                                    let targets: Vec<TokenStream> = alias.get("targets").and_then(|t| t.as_array()).map(|arr| {
                                        arr.iter().map(|t| {
                                            let data = generate_identifier(t.get("data").and_then(|d| d.as_str()).unwrap_or(""));
                                            let w = t.get("weight").and_then(|w| w.as_i64()).unwrap_or(1) as i32;
                                            quote! { (#data, #w) }
                                        }).collect()
                                    }).unwrap_or_default();
                                    quote! { PoolAlias::Random { alias: #a, targets: vec![#(#targets),*] } }
                                }
                                "minecraft:random_group" => {
                                    let groups: Vec<TokenStream> = alias.get("groups").and_then(|g| g.as_array()).map(|arr| {
                                        arr.iter().map(|group| {
                                            let w = group.get("weight").and_then(|w| w.as_i64()).unwrap_or(1) as i32;
                                            let bindings: Vec<TokenStream> = group.get("data").and_then(|d| d.as_array()).map(|data| {
                                                data.iter().map(|b| {
                                                    let a = generate_identifier(b.get("alias").and_then(|v| v.as_str()).unwrap_or(""));
                                                    let t = generate_identifier(b.get("target").and_then(|v| v.as_str()).unwrap_or(""));
                                                    quote! { (#a, #t) }
                                                }).collect()
                                            }).unwrap_or_default();
                                            quote! { (vec![#(#bindings),*], #w) }
                                        }).collect()
                                    }).unwrap_or_default();
                                    quote! { PoolAlias::RandomGroup { groups: vec![#(#groups),*] } }
                                }
                                _ => quote! { },
                            }
                        }).collect();

                        quote! {
                            Some(JigsawConfig {
                                start_pool: #start_pool,
                                max_depth: #max_depth,
                                use_expansion_hack: #use_expansion_hack,
                                project_start_to_heightmap: #heightmap_token,
                                start_height: #start_height_token,
                                max_distance_from_center: #max_dist,
                                start_jigsaw_name: #jigsaw_name_token,
                                dimension_padding: DimensionPadding { bottom: #pad_bottom, top: #pad_top },
                                terrain_adaptation: #terrain.to_string(),
                                pool_aliases: vec![#(#alias_tokens),*],
                            })
                        }
                    }
                    None => quote! { None },
                };

                quote! {
                    StructureEntryData {
                        structure: #structure,
                        weight: #weight,
                        allowed_biomes: vec![#(#biome_tokens),*],
                        biome_check_y: #biome_check_y_token,
                        structure_type: #stype.to_string(),
                        jigsaw_config: #jigsaw_config_token,
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
            JigsawConfig, StartHeight, DimensionPadding, PoolAlias,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;

        /// Returns all vanilla structure sets parsed from the datapack.
        pub fn vanilla_structure_sets() -> Vec<StructureSetData> {
            vec![#entries]
        }
    }
}
