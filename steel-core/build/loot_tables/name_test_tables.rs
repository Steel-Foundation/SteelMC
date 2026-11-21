//! Exercises source map names independently of structure lookup.
use std::path::Path;

use serde_json::{Value, json};

use super::{Compiler, INPUT_ROOT};

pub(super) fn inputs() -> impl IntoIterator<Item = (&'static str, Value)> {
    let camp: Value = Compiler::read_input(
        &Path::new(INPUT_ROOT).join("loot_table/chests/abandoned_camp_common_chest.json"),
    );
    let names = camp["pools"]
        .as_array()
        .expect("camp pools")
        .iter()
        .flat_map(|pool| pool["entries"].as_array().expect("camp entries"))
        .filter(|entry| entry["name"] == "minecraft:abandoned_camp_map")
        .map(|entry| {
            entry["modifier"]
                .as_array()
                .expect("camp modifiers")
                .iter()
                .find(|modifier| modifier["type"] == "minecraft:set_name")
                .expect("camp name")
        })
        .collect::<Vec<_>>();
    let first_name = names.first().expect("source map names");
    let pools = names
        .iter()
        .map(|name| {
            json!({"rolls":1,"entries":[{
                "type":"minecraft:item","name":"minecraft:abandoned_camp_map","modifier":name
            }]})
        })
        .collect::<Vec<_>>();
    let shipwreck: Value =
        Compiler::read_input(&Path::new(INPUT_ROOT).join("loot_table/chests/shipwreck_map.json"));
    let exploration = shipwreck["pools"]
        .as_array()
        .expect("shipwreck pools")
        .iter()
        .flat_map(|pool| pool["entries"].as_array().expect("shipwreck entries"))
        .find(|entry| entry["name"] == "minecraft:buried_treasure_map")
        .expect("shipwreck map")["modifier"]
        .as_array()
        .expect("map modifiers")
        .iter()
        .find(|modifier| modifier["type"] == "minecraft:exploration_map")
        .expect("source exploration modifier");
    [
        ("SET_NAME_CAMP_MAPS", json!({"pools":pools})),
        (
            "SET_NAME_TARGETS",
            table(json!([
                {"type":"minecraft:set_name","name":"before"},
                {"type":"minecraft:set_name","target":"item_name","name":{
                    "translate":"filled_map.bamboo_camp_map","fallback":"Camp %s",
                    "with":[{"text":"argument","extra":[{"translate":"filled_map.map"}]}],
                    "extra":[{"text":"suffix"}]
                }},
                {"type":"minecraft:set_name","name":{"text":"after"}},
                {"type":"minecraft:set_name"},
                {"type":"minecraft:set_name","name":"hidden","condition":{"type":"minecraft:random_chance","chance":0.0}}
            ])),
        ),
        (
            "SET_NAME_ABSENT",
            table(json!({"type":"minecraft:set_name"})),
        ),
        (
            "SET_NAME_EXPLORATION_MAP",
            json!({"pools":[{"rolls":1,"entries":[{
                "type":"minecraft:item","name":"minecraft:buried_treasure_map",
                "modifier":[first_name,exploration]
            }]}]}),
        ),
    ]
}

fn table(modifier: Value) -> Value {
    json!({"pools":[{"rolls":1,"entries":[{
        "type":"minecraft:item","name":"minecraft:map","modifier":modifier
    }]}]})
}
