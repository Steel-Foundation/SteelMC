use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct DimensionTypeJson {
    #[serde(default)]
    attributes: DimensionAttributes,

    #[serde(default)]
    has_fixed_time: bool,
    #[serde(default)]
    fixed_time: Option<i64>,
    has_skylight: bool,
    has_ceiling: bool,
    coordinate_scale: f64,
    min_y: i32,
    height: i32,
    logical_height: i32,
    infiniburn: String,
    ambient_light: f32,
    monster_spawn_light_level: MonsterSpawnLightLevelJson,
    monster_spawn_block_light_limit: i32,

    #[serde(default)]
    #[allow(dead_code)]
    skybox: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    timelines: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    cardinal_light: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
struct DimensionAttributes {
    #[serde(rename = "minecraft:gameplay/respawn_anchor_works")]
    respawn_anchor_works: Option<bool>,
    #[serde(rename = "minecraft:gameplay/can_start_raid")]
    can_start_raid: Option<bool>,
    #[serde(rename = "minecraft:visual/cloud_height")]
    cloud_height: Option<f64>,
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
pub enum MonsterSpawnLightLevelJson {
    Simple(i32),
    Complex {
        #[serde(rename = "type")]
        distribution_type: String,
        min_inclusive: i32,
        max_inclusive: i32,
    },
}

fn generate_option<T, F>(opt: &Option<T>, f: F) -> TokenStream
where
    F: FnOnce(&T) -> TokenStream,
{
    match opt {
        Some(val) => {
            let inner = f(val);
            quote! { Some(#inner) }
        }
        None => quote! { None },
    }
}

fn generate_monster_spawn_light_level(level: &MonsterSpawnLightLevelJson) -> TokenStream {
    match level {
        MonsterSpawnLightLevelJson::Simple(value) => {
            quote! { MonsterSpawnLightLevel::Simple(#value) }
        }
        MonsterSpawnLightLevelJson::Complex {
            distribution_type,
            min_inclusive,
            max_inclusive,
        } => {
            let dist_type = distribution_type.as_str();
            quote! {
                MonsterSpawnLightLevel::Complex {
                    distribution_type: #dist_type,
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/dimension_type/"
    );

    let dimension_type_dir =
        "build_assets/builtin_datapacks/minecraft/data/minecraft/dimension_type";
    let mut dimension_types = Vec::new();

    // Read all dimension type JSON files
    for entry in fs::read_dir(dimension_type_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let dimension_type_name = path.file_stem().unwrap().to_str().unwrap().to_string();
            let content = fs::read_to_string(&path).unwrap();
            let mut dimension_type: DimensionTypeJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("Failed to parse {}: {}", dimension_type_name, e));

            // Extract fixed_time from attributes if has_fixed_time is true but fixed_time is None
            if dimension_type.has_fixed_time && dimension_type.fixed_time.is_none() {
                // Try to extract from attributes, or use a default
                // For the_end, it's 6000, for nether it might be different
                dimension_type.fixed_time = match dimension_type_name.as_str() {
                    "the_end" => Some(6000),
                    "the_nether" => Some(18000),
                    _ => None,
                };
            }

            dimension_types.push((dimension_type_name, dimension_type));
        }
    }

    // Sort dimension types by name for consistent generation
    dimension_types.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::dimension_type::{
            DimensionType, DimensionTypeRegistry, MonsterSpawnLightLevel,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static dimension type definitions
    for (dimension_type_name, dimension_type) in &dimension_types {
        let dimension_type_ident = Ident::new(
            &dimension_type_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        let dimension_type_name_str = dimension_type_name.clone();

        let key = quote! { Identifier::vanilla_static(#dimension_type_name_str) };
        let fixed_time = generate_option(&dimension_type.fixed_time, |t| quote! { #t });
        let has_skylight = dimension_type.has_skylight;
        let has_ceiling = dimension_type.has_ceiling;

        // Extract values from attributes
        let respawn_anchor_works = dimension_type
            .attributes
            .respawn_anchor_works
            .unwrap_or(false);
        let has_raids = dimension_type.attributes.can_start_raid.unwrap_or(true);
        let cloud_height = generate_option(
            &dimension_type.attributes.cloud_height.map(|h| h as i32),
            |h| quote! { #h },
        );

        let coordinate_scale = dimension_type.coordinate_scale;
        let min_y = dimension_type.min_y;
        let height = dimension_type.height;
        let logical_height = dimension_type.logical_height;
        let infiniburn = dimension_type.infiniburn.as_str();
        let ambient_light = dimension_type.ambient_light;
        let monster_spawn_light_level =
            generate_monster_spawn_light_level(&dimension_type.monster_spawn_light_level);
        let monster_spawn_block_light_limit = dimension_type.monster_spawn_block_light_limit;

        stream.extend(quote! {
            pub static #dimension_type_ident: &DimensionType = &DimensionType {
                key: #key,
                fixed_time: #fixed_time,
                has_skylight: #has_skylight,
                has_ceiling: #has_ceiling,
                coordinate_scale: #coordinate_scale,
                respawn_anchor_works: #respawn_anchor_works,
                min_y: #min_y,
                height: #height,
                logical_height: #logical_height,
                infiniburn: #infiniburn,
                ambient_light: #ambient_light,
                cloud_height: #cloud_height,
                has_raids: #has_raids,
                monster_spawn_light_level: #monster_spawn_light_level,
                monster_spawn_block_light_limit: #monster_spawn_block_light_limit,
            };
        });
    }

    // Generate registration function
    let mut register_stream = TokenStream::new();
    for (dimension_type_name, _) in &dimension_types {
        let dimension_type_ident = Ident::new(
            &dimension_type_name.to_shouty_snake_case(),
            Span::call_site(),
        );
        register_stream.extend(quote! {
            registry.register(#dimension_type_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_dimension_types(registry: &mut DimensionTypeRegistry) {
            #register_stream
        }
    });

    stream
}
