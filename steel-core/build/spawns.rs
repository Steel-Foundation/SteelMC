use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Debug, Deserialize)]
struct BiomeFile {
    #[serde(default)]
    spawners: std::collections::BTreeMap<String, Vec<Spawner>>,
}

#[derive(Debug, Deserialize)]
struct Spawner {
    #[serde(rename = "type")]
    entity_type: String,
    weight: i32,
    #[serde(rename = "minCount")]
    min_count: i32,
    #[serde(rename = "maxCount")]
    max_count: i32,
}

pub fn build(manifest_dir: &str) -> String {
    let root = Path::new(manifest_dir).join("../generated/data/minecraft/worldgen/biome");
    let mut biomes = Vec::new();
    for file in fs::read_dir(&root).unwrap_or_else(|error| {
        panic!("failed to read biome directory {}: {error}", root.display())
    }) {
        let file = file.expect("failed to read biome file");
        let path = file.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .expect("biome filename")
            .to_string_lossy()
            .into_owned();
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let biome: BiomeFile = serde_json::from_str(&raw)
            .unwrap_or_else(|error| panic!("invalid biome {}: {error}", path.display()));
        biomes.push((name, biome));
    }
    biomes.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = String::from(
        "#[derive(Debug, Clone, Copy)]\npub struct SpawnData { pub category: &'static str, pub entity_type: &'static str, pub weight: i32, pub min_count: i32, pub max_count: i32 }\n#[derive(Debug, Clone, Copy)]\npub struct BiomeSpawnData { pub biome: &'static str, pub spawns: &'static [SpawnData] }\n\npub static BIOME_SPAWNS: &[BiomeSpawnData] = &[\n",
    );
    for (name, biome) in biomes {
        let mut entries = Vec::new();
        for (category, spawns) in biome.spawners {
            for spawn in spawns {
                assert!(
                    spawn.weight > 0 && spawn.min_count > 0 && spawn.max_count >= spawn.min_count,
                    "invalid spawn range in biome {name}"
                );
                entries.push(format!("SpawnData {{ category: {:?}, entity_type: {:?}, weight: {}, min_count: {}, max_count: {} }}", category, spawn.entity_type, spawn.weight, spawn.min_count, spawn.max_count));
            }
        }
        out.push_str(&format!(
            "    BiomeSpawnData {{ biome: {:?}, spawns: &[{}] }},\n",
            name,
            entries.join(", ")
        ));
    }
    out.push_str("];\n");
    out
}
