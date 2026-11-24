//! Isolates camp-map eligibility from the other modifiers in the source table.
use std::path::Path;

use serde_json::{Value, json};

use super::{Compiler, INPUT_ROOT};

pub(super) fn inputs() -> impl IntoIterator<Item = (&'static str, Value)> {
    let source: Value = Compiler::read_input(
        &Path::new(INPUT_ROOT).join("loot_table/chests/abandoned_camp_common_chest.json"),
    );
    let pools = source["pools"]
        .as_array()
        .expect("camp pools")
        .iter()
        .flat_map(|pool| pool["entries"].as_array().expect("camp entries"))
        .filter(|entry| entry["name"] == "minecraft:abandoned_camp_map")
        .enumerate()
        .map(|(index, entry)| {
            json!({"rolls":1,"entries":[{
                "type":"minecraft:item", "name":"minecraft:map",
                "condition":entry["condition"],
                "modifier":{"type":"minecraft:set_count","count":index + 1}
            }]})
        })
        .collect::<Vec<_>>();
    assert!(!pools.is_empty(), "camp map conditions must be exercised");
    [
        ("LOCATION_CAMP_BIOMES", json!({"pools":pools})),
        (
            "LOCATION_BIOME",
            table(json!({"predicate":{"biomes":"minecraft:bamboo_jungle"}})),
        ),
        (
            "LOCATION_BIOME_TAG",
            table(json!({"predicate":{"biomes":"#minecraft:is_jungle"}})),
        ),
        (
            "LOCATION_BIOME_LIST",
            table(
                json!({"predicate":{"biomes":["minecraft:bamboo_jungle","minecraft:cherry_grove"]}}),
            ),
        ),
        (
            "LOCATION_BIOME_EMPTY_LIST",
            table(json!({"predicate":{"biomes":[]}})),
        ),
        (
            "LOCATION_OFFSET",
            table(
                json!({"offsetX":-16,"offsetY":1,"offsetZ":16,"predicate":{"biomes":"minecraft:bamboo_jungle"}}),
            ),
        ),
        ("LOCATION_NO_PREDICATE", table(json!({}))),
        ("LOCATION_EMPTY_PREDICATE", table(json!({"predicate":{}}))),
        (
            "LOCATION_UNSUPPORTED",
            table(json!({"predicate":{"biomes":"minecraft:bamboo_jungle","block":{}}})),
        ),
    ]
}

fn table(mut condition: Value) -> Value {
    condition["type"] = json!("minecraft:location_check");
    json!({"pools":[{"rolls":1,"entries":[{
        "type":"minecraft:item","name":"minecraft:map","condition":condition
    }]}]})
}
