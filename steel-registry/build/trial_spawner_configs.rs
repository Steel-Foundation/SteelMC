use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

const TRIAL_SPAWNER_DIR: &str =
    "../steel-utils/build_assets/builtin_datapacks/minecraft/trial_spawner";

#[derive(Debug, Deserialize)]
struct ConfigJson {
    #[serde(default)]
    spawn_range: Option<i32>,
    #[serde(default)]
    total_mobs: Option<f32>,
    #[serde(default)]
    simultaneous_mobs: Option<f32>,
    #[serde(default)]
    total_mobs_added_per_player: Option<f32>,
    #[serde(default)]
    simultaneous_mobs_added_per_player: Option<f32>,
    #[serde(default)]
    ticks_between_spawn: Option<i32>,
    #[serde(default)]
    spawn_potentials: Vec<WeightedSpawnPotentialJson>,
    #[serde(default)]
    loot_tables_to_eject: Option<Vec<WeightedLootTableJson>>,
    #[serde(default)]
    items_to_drop_when_ominous: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WeightedSpawnPotentialJson {
    data: SpawnPotentialDataJson,
    weight: u32,
}

#[derive(Debug, Deserialize)]
struct SpawnPotentialDataJson {
    entity: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    equipment: Option<EquipmentJson>,
}

#[derive(Debug, Deserialize)]
struct EquipmentJson {
    loot_table: String,
    #[serde(default)]
    slot_drop_chances: f32,
}

#[derive(Debug, Deserialize)]
struct WeightedLootTableJson {
    data: String,
    weight: u32,
}

/// Extra entity tags found in the extracted configs. Extend as needed; keep
/// values restricted to what vanilla actually serializes into `SpawnData`.
enum ExtraTag {
    Int(String, i32),
    Str(String, String),
}

fn extra_tags(entity: &serde_json::Map<String, serde_json::Value>) -> Vec<ExtraTag> {
    entity
        .iter()
        .filter(|(key, _)| key.as_str() != "id")
        .filter_map(|(key, value)| match value {
            serde_json::Value::Number(n) if n.is_i64() => {
                Some(ExtraTag::Int(key.clone(), n.as_i64().unwrap_or(0) as i32))
            }
            serde_json::Value::String(s) => Some(ExtraTag::Str(key.clone(), s.clone())),
            other => panic!("unsupported trial spawner entity tag {key}: {other}"),
        })
        .collect()
}

fn collect_configs(dir: &std::path::Path, prefix: &str, out: &mut Vec<(String, ConfigJson)>) {
    for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {dir:?}: {e}")) {
        let path = entry.expect("dir entry").path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if path.is_dir() {
            let sub_prefix = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            collect_configs(&path, &sub_prefix, out);
        } else if path.extension().is_some_and(|ext| ext == "json") {
            let key = if prefix.is_empty() {
                path.file_stem().unwrap().to_string_lossy().to_string()
            } else {
                format!("{prefix}/{}", path.file_stem().unwrap().to_string_lossy())
            };
            let content = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let config: ConfigJson = serde_json::from_str(&content)
                .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
            out.push((key, config));
        }
    }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed={TRIAL_SPAWNER_DIR}");
    let dir = std::path::Path::new(TRIAL_SPAWNER_DIR);
    let mut found: Vec<(String, ConfigJson)> = Vec::new();
    collect_configs(dir, "", &mut found);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!found.is_empty(), "no trial spawner configs extracted");

    let mut definitions = TokenStream::new();
    let mut registrations = TokenStream::new();
    for (key, config) in &found {
        let ident = Ident::new(
            &key.replace('/', "_").to_shouty_snake_case(),
            Span::call_site(),
        );

        let potentials = config.spawn_potentials.iter().map(|potential| {
            let weight = potential.weight;
            let entity_id = potential
                .data
                .entity
                .get("id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_else(|| panic!("spawn potential for {key} missing entity id"));
            let tags = extra_tags(&potential.data.entity).into_iter().map(|tag| match tag {
                ExtraTag::Int(name, value) => {
                    let name = name.as_str();
                    quote! { TrialSpawnerEntityTag::Int { name: #name, value: #value } }
                }
                ExtraTag::Str(name, value) => {
                    let name = name.as_str();
                    let value = value.as_str();
                    quote! { TrialSpawnerEntityTag::String { name: #name, value: #value } }
                }
            });
            let equipment = match &potential.data.equipment {
                Some(equipment) => {
                    let loot_table = equipment.loot_table.as_str();
                    let chances = equipment.slot_drop_chances;
                    quote! { Some(TrialSpawnerEquipment { loot_table: #loot_table, slot_drop_chances: #chances }) }
                }
                None => quote! { None },
            };
            quote! {
                TrialSpawnerSpawnPotential {
                    weight: #weight,
                    entity_id: #entity_id,
                    extra_tags: &[#(#tags),*],
                    equipment: #equipment,
                }
            }
        });

        let eject_tables = config
            .loot_tables_to_eject
            .iter()
            .flatten()
            .map(|entry| {
                let weight = entry.weight;
                let loot_table = entry.data.as_str();
                quote! {
                    TrialSpawnerEjectLootTable { weight: #weight, loot_table: #loot_table }
                }
            });

        let items_to_drop = config
            .items_to_drop_when_ominous
            .as_deref()
            .unwrap_or("minecraft:spawners/trial_chamber/items_to_drop_when_ominous");

        let spawn_range = config.spawn_range.unwrap_or(4);
        let total_mobs = config.total_mobs.unwrap_or(6.0);
        let simultaneous_mobs = config.simultaneous_mobs.unwrap_or(2.0);
        let total_mobs_added_per_player = config.total_mobs_added_per_player.unwrap_or(2.0);
        let simultaneous_mobs_added_per_player =
            config.simultaneous_mobs_added_per_player.unwrap_or(1.0);
        let ticks_between_spawn = config.ticks_between_spawn.unwrap_or(40);

        let key = key.as_str();
        definitions.extend(quote! {
            static #ident: TrialSpawnerConfig = TrialSpawnerConfig::new(
                Identifier::vanilla_static(#key),
                #spawn_range,
                #total_mobs,
                #simultaneous_mobs,
                #total_mobs_added_per_player,
                #simultaneous_mobs_added_per_player,
                #ticks_between_spawn,
                &[#(#potentials),*],
                &[#(#eject_tables),*],
                #items_to_drop,
            );
        });
        registrations.extend(quote! { registry.register(&#ident); });
    }

    quote! {
        use crate::trial_spawner::{
            TrialSpawnerConfig, TrialSpawnerConfigRegistry, TrialSpawnerEjectLootTable,
            TrialSpawnerEquipment, TrialSpawnerSpawnPotential, TrialSpawnerEntityTag,
        };
        use steel_utils::Identifier;

        #definitions

        pub fn register_trial_spawner_configs(registry: &mut TrialSpawnerConfigRegistry) {
            #registrations
        }
    }
}
