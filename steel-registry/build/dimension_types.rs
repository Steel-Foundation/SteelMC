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
    has_ender_dragon_fight: bool,

    #[serde(default)]
    skybox: Option<String>,
    #[serde(default)]
    timelines: Option<String>,
    #[serde(default)]
    default_clock: Option<String>,
    #[serde(default)]
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
    #[serde(rename = "minecraft:visual/sky_color")]
    sky_color: Option<String>,
    #[serde(rename = "minecraft:visual/fog_color")]
    fog_color: Option<String>,
    #[serde(rename = "minecraft:visual/cloud_color")]
    cloud_color: Option<String>,

    // New visual attributes
    #[serde(rename = "minecraft:visual/ambient_light_color")]
    ambient_light_color: Option<String>,
    #[serde(rename = "minecraft:visual/sky_light_color")]
    sky_light_color: Option<String>,
    #[serde(rename = "minecraft:visual/sky_light_factor")]
    sky_light_factor: Option<f32>,
    #[serde(rename = "minecraft:visual/fog_start_distance")]
    fog_start_distance: Option<f32>,
    #[serde(rename = "minecraft:visual/fog_end_distance")]
    fog_end_distance: Option<f32>,
    #[serde(rename = "minecraft:visual/default_dripstone_particle")]
    default_dripstone_particle: Option<DripstoneParticleJson>,

    // New gameplay attributes
    #[serde(rename = "minecraft:gameplay/fast_lava")]
    fast_lava: Option<bool>,
    #[serde(rename = "minecraft:gameplay/piglins_zombify")]
    piglins_zombify: Option<bool>,
    #[serde(rename = "minecraft:gameplay/sky_light_level")]
    sky_light_level: Option<f32>,
    #[serde(rename = "minecraft:gameplay/snow_golem_melts")]
    snow_golem_melts: Option<bool>,
    #[serde(rename = "minecraft:gameplay/water_evaporates")]
    water_evaporates: Option<bool>,
    #[serde(rename = "minecraft:gameplay/nether_portal_spawns_piglin")]
    nether_portal_spawns_piglin: Option<bool>,
    #[serde(rename = "minecraft:gameplay/bed_rule")]
    bed_rule: Option<BedRuleJson>,

    // Audio attributes
    #[serde(rename = "minecraft:audio/ambient_sounds")]
    ambient_sounds: Option<AmbientSoundsJson>,
    #[serde(rename = "minecraft:audio/background_music")]
    background_music: Option<BackgroundMusicJson>,
}

#[derive(Deserialize, Debug)]
struct BedRuleJson {
    can_set_spawn: String,
    can_sleep: String,
    #[serde(default)]
    explodes: bool,
    error_message: Option<ErrorMessageJson>,
}

#[derive(Deserialize, Debug)]
struct ErrorMessageJson {
    translate: String,
}

#[derive(Deserialize, Debug)]
struct AmbientSoundsJson {
    mood: MoodJson,
}

#[derive(Deserialize, Debug)]
struct MoodJson {
    sound: String,
    tick_delay: i32,
    block_search_extent: i32,
    offset: f64,
}

#[derive(Deserialize, Debug)]
struct BackgroundMusicJson {
    default: MusicEntryJson,
    #[serde(default)]
    creative: Option<MusicEntryJson>,
}

#[derive(Deserialize, Debug)]
struct MusicEntryJson {
    sound: String,
    min_delay: i32,
    max_delay: i32,
    #[serde(default)]
    replace_current_music: bool,
}

#[derive(Deserialize, Debug)]
struct DripstoneParticleJson {
    #[serde(rename = "type")]
    particle_type: String,
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

fn generate_bed_rule(bed_rule: &BedRuleJson) -> TokenStream {
    let can_set_spawn = bed_rule.can_set_spawn.as_str();
    let can_sleep = bed_rule.can_sleep.as_str();
    let explodes = bed_rule.explodes;
    let error_message_key = generate_option(
        &bed_rule.error_message.as_ref().map(|m| m.translate.clone()),
        |s| {
            let s = s.as_str();
            quote! { #s }
        },
    );
    quote! {
        BedRule {
            can_set_spawn: #can_set_spawn,
            can_sleep: #can_sleep,
            explodes: #explodes,
            error_message_key: #error_message_key,
        }
    }
}

fn generate_mood_sound(mood: &MoodJson) -> TokenStream {
    let sound = mood.sound.as_str();
    let tick_delay = mood.tick_delay;
    let block_search_extent = mood.block_search_extent;
    let offset = mood.offset;
    quote! {
        MoodSound {
            sound: #sound,
            tick_delay: #tick_delay,
            block_search_extent: #block_search_extent,
            offset: #offset,
        }
    }
}

fn generate_music_entry(entry: &MusicEntryJson) -> TokenStream {
    let sound = entry.sound.as_str();
    let min_delay = entry.min_delay;
    let max_delay = entry.max_delay;
    let replace_current_music = entry.replace_current_music;
    quote! {
        MusicEntry {
            sound: #sound,
            min_delay: #min_delay,
            max_delay: #max_delay,
            replace_current_music: #replace_current_music,
        }
    }
}

fn generate_background_music(bg: &BackgroundMusicJson) -> TokenStream {
    let default_entry = generate_music_entry(&bg.default);
    let creative = generate_option(&bg.creative, generate_music_entry);
    quote! {
        BackgroundMusic {
            default: #default_entry,
            creative: #creative,
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
            BedRule, MoodSound, MusicEntry, BackgroundMusic,
        };
        use steel_utils::Identifier;
        use std::borrow::Cow;
    });

    // Generate static dimension type definitions
    let mut register_stream = TokenStream::new();
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

        // Top-level optional fields
        let skybox = generate_option(&dimension_type.skybox.as_deref().map(str::to_owned), |s| {
            let s = s.as_str();
            quote! { #s }
        });
        let cardinal_light = generate_option(
            &dimension_type.cardinal_light.as_deref().map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let default_clock = generate_option(
            &dimension_type.default_clock.as_deref().map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let timelines = generate_option(
            &dimension_type.timelines.as_deref().map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );

        // Visual attributes
        let sky_color = generate_option(
            &dimension_type
                .attributes
                .sky_color
                .as_deref()
                .map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let fog_color = generate_option(
            &dimension_type
                .attributes
                .fog_color
                .as_deref()
                .map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let cloud_color = generate_option(
            &dimension_type
                .attributes
                .cloud_color
                .as_deref()
                .map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let cloud_height = generate_option(
            &dimension_type.attributes.cloud_height.map(|h| h as f32),
            |h| quote! { #h },
        );
        let ambient_light_color = generate_option(
            &dimension_type
                .attributes
                .ambient_light_color
                .as_deref()
                .map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let sky_light_color = generate_option(
            &dimension_type
                .attributes
                .sky_light_color
                .as_deref()
                .map(str::to_owned),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );
        let sky_light_factor = generate_option(
            &dimension_type.attributes.sky_light_factor,
            |v| quote! { #v },
        );
        let fog_start_distance = generate_option(
            &dimension_type.attributes.fog_start_distance,
            |v| quote! { #v },
        );
        let fog_end_distance = generate_option(
            &dimension_type.attributes.fog_end_distance,
            |v| quote! { #v },
        );
        let default_dripstone_particle = generate_option(
            &dimension_type
                .attributes
                .default_dripstone_particle
                .as_ref()
                .map(|p| p.particle_type.clone()),
            |s| {
                let s = s.as_str();
                quote! { #s }
            },
        );

        // Gameplay attributes
        let respawn_anchor_works = dimension_type
            .attributes
            .respawn_anchor_works
            .unwrap_or(false);
        let can_start_raid = dimension_type.attributes.can_start_raid.unwrap_or(true);
        let fast_lava = dimension_type.attributes.fast_lava.unwrap_or(false);
        let piglins_zombify = dimension_type.attributes.piglins_zombify.unwrap_or(true);
        let sky_light_level = generate_option(
            &dimension_type.attributes.sky_light_level,
            |v| quote! { #v },
        );
        let snow_golem_melts = dimension_type.attributes.snow_golem_melts.unwrap_or(false);
        let water_evaporates = dimension_type.attributes.water_evaporates.unwrap_or(false);
        let nether_portal_spawns_piglin = dimension_type
            .attributes
            .nether_portal_spawns_piglin
            .unwrap_or(false);

        let bed_rule = generate_bed_rule(
            dimension_type
                .attributes
                .bed_rule
                .as_ref()
                .unwrap_or_else(|| panic!("Missing bed_rule in {}", dimension_type_name)),
        );

        // Audio attributes
        let mood_sound = generate_option(&dimension_type.attributes.ambient_sounds, |s| {
            generate_mood_sound(&s.mood)
        });
        let background_music = generate_option(&dimension_type.attributes.background_music, |bg| {
            generate_background_music(bg)
        });

        let coordinate_scale = dimension_type.coordinate_scale;
        let min_y = dimension_type.min_y;
        let height = dimension_type.height;
        let logical_height = dimension_type.logical_height;
        let infiniburn = dimension_type.infiniburn.as_str();
        let ambient_light = dimension_type.ambient_light;
        let monster_spawn_light_level =
            generate_monster_spawn_light_level(&dimension_type.monster_spawn_light_level);
        let monster_spawn_block_light_limit = dimension_type.monster_spawn_block_light_limit;
        let has_ender_dragon_fight = dimension_type.has_ender_dragon_fight;

        stream.extend(quote! {
            pub static #dimension_type_ident: &DimensionType = &DimensionType {
                key: #key,
                fixed_time: #fixed_time,
                has_skylight: #has_skylight,
                has_ceiling: #has_ceiling,
                coordinate_scale: #coordinate_scale,
                min_y: #min_y,
                height: #height,
                logical_height: #logical_height,
                infiniburn: #infiniburn,
                ambient_light: #ambient_light,
                default_clock: #default_clock,
                timelines: #timelines,
                has_ender_dragon_fight: #has_ender_dragon_fight,
                monster_spawn_light_level: #monster_spawn_light_level,
                monster_spawn_block_light_limit: #monster_spawn_block_light_limit,
                skybox: #skybox,
                cardinal_light: #cardinal_light,
                sky_color: #sky_color,
                fog_color: #fog_color,
                cloud_color: #cloud_color,
                cloud_height: #cloud_height,
                ambient_light_color: #ambient_light_color,
                sky_light_color: #sky_light_color,
                sky_light_factor: #sky_light_factor,
                fog_start_distance: #fog_start_distance,
                fog_end_distance: #fog_end_distance,
                default_dripstone_particle: #default_dripstone_particle,
                respawn_anchor_works: #respawn_anchor_works,
                can_start_raid: #can_start_raid,
                fast_lava: #fast_lava,
                piglins_zombify: #piglins_zombify,
                sky_light_level: #sky_light_level,
                snow_golem_melts: #snow_golem_melts,
                water_evaporates: #water_evaporates,
                nether_portal_spawns_piglin: #nether_portal_spawns_piglin,
                bed_rule: #bed_rule,
                mood_sound: #mood_sound,
                background_music: #background_music,
            };
        });

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
