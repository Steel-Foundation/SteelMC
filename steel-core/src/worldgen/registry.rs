//! Runtime registry for world generator factories.

use rustc_hash::FxHashMap;
use serde::Deserialize;
use std::iter::repeat_n;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::vanilla_dimension_types::{OVERWORLD, THE_END, THE_NETHER};
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use toml::map::Map;

use crate::worldgen::{
    BiomeSourceKind, ChunkGeneratorType, EmptyChunkGenerator, FlatChunkGenerator, VanillaGenerator,
};

/// Fully constructed generator metadata for a world.
pub struct GeneratorOutput {
    /// Vanilla dimension type rules used by this loaded world.
    pub dimension_type: DimensionTypeRef,
    /// Generator config after applying generator defaults.
    pub config: toml::Value,
    /// Chunk generator instance.
    pub generator: ChunkGeneratorType,
    /// Whether the client should treat this as a flat world.
    pub is_flat: bool,
    /// Sea level sent in login/respawn packets.
    pub sea_level: i32,
}

struct WorldGeneratorFactory {
    validate: fn(&toml::Value) -> Result<(), String>,
    create: fn(&toml::Value, i64) -> Result<GeneratorOutput, String>,
}

/// Registry of server-side world generator factories.
pub struct WorldGeneratorRegistry {
    factories: FxHashMap<Identifier, WorldGeneratorFactory>,
}

impl WorldGeneratorRegistry {
    /// Creates a registry containing Steel's built-in generator factories.
    pub fn new_with_builtins() -> Result<Self, String> {
        let mut registry = Self {
            factories: FxHashMap::default(),
        };

        registry.register(
            Identifier::vanilla_static("overworld"),
            WorldGeneratorFactory {
                validate: validate_empty_config,
                create: create_overworld,
            },
        )?;
        registry.register(
            Identifier::vanilla_static("the_nether"),
            WorldGeneratorFactory {
                validate: validate_empty_config,
                create: create_nether,
            },
        )?;
        registry.register(
            Identifier::vanilla_static("the_end"),
            WorldGeneratorFactory {
                validate: validate_empty_config,
                create: create_end,
            },
        )?;
        registry.register(
            Identifier::vanilla_static("flat"),
            WorldGeneratorFactory {
                validate: validate_flat_config,
                create: create_flat,
            },
        )?;
        registry.register(
            Identifier::new("steel", "empty"),
            WorldGeneratorFactory {
                validate: validate_empty_world_config,
                create: create_empty,
            },
        )?;

        Ok(registry)
    }

    fn register(&mut self, key: Identifier, factory: WorldGeneratorFactory) -> Result<(), String> {
        if self.factories.insert(key.clone(), factory).is_some() {
            return Err(format!("duplicate world generator registration {key}"));
        }
        Ok(())
    }

    /// Validates config for a generator ID.
    pub fn validate_config(&self, key: &Identifier, config: &toml::Value) -> Result<(), String> {
        let factory = self
            .factories
            .get(key)
            .ok_or_else(|| format!("unknown world generator {key}"))?;
        (factory.validate)(config)
    }

    /// Creates a generator from a validated generator ID and config.
    pub fn create(
        &self,
        key: &Identifier,
        config: &toml::Value,
        seed: i64,
    ) -> Result<GeneratorOutput, String> {
        let factory = self
            .factories
            .get(key)
            .ok_or_else(|| format!("unknown world generator {key}"))?;
        (factory.create)(config, seed)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DimensionTypeOnlyConfig {
    dimension_type: Identifier,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlatGeneratorConfig {
    #[serde(default = "default_flat_dimension_type")]
    dimension_type: Identifier,
    #[serde(default = "default_flat_layers")]
    layers: Vec<FlatLayerConfig>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FlatLayerConfig {
    block: Identifier,
    height: usize,
}

const fn default_flat_dimension_type() -> Identifier {
    Identifier::vanilla_static("overworld")
}

fn default_flat_layers() -> Vec<FlatLayerConfig> {
    vec![
        FlatLayerConfig {
            block: Identifier::vanilla_static("bedrock"),
            height: 1,
        },
        FlatLayerConfig {
            block: Identifier::vanilla_static("dirt"),
            height: 2,
        },
        FlatLayerConfig {
            block: Identifier::vanilla_static("grass_block"),
            height: 1,
        },
    ]
}

fn validate_empty_config(config: &toml::Value) -> Result<(), String> {
    let Some(table) = config.as_table() else {
        return Err("generator config must be a table".to_owned());
    };
    if !table.is_empty() {
        return Err("this generator does not accept config".to_owned());
    }
    Ok(())
}

fn validate_empty_world_config(config: &toml::Value) -> Result<(), String> {
    let parsed: DimensionTypeOnlyConfig = config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid steel:empty config: {e}"))?;
    dimension_type_by_key(&parsed.dimension_type).map(|_| ())
}

fn validate_flat_config(config: &toml::Value) -> Result<(), String> {
    let parsed = parse_flat_config(config)?;
    if parsed.layers.is_empty() {
        return Err("minecraft:flat requires at least one layer".to_owned());
    }
    dimension_type_by_key(&parsed.dimension_type)?;
    for layer in &parsed.layers {
        if layer.height == 0 {
            return Err("minecraft:flat layer height must be greater than zero".to_owned());
        }
        if REGISTRY.blocks.by_key(&layer.block).is_none() {
            return Err(format!(
                "unknown block {} in minecraft:flat layer",
                layer.block
            ));
        }
    }
    Ok(())
}

fn parse_flat_config(config: &toml::Value) -> Result<FlatGeneratorConfig, String> {
    config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid minecraft:flat config: {e}"))
}

fn create_overworld(config: &toml::Value, seed: i64) -> Result<GeneratorOutput, String> {
    validate_empty_config(config)?;
    let seed = seed as u64;
    Ok(GeneratorOutput {
        dimension_type: &OVERWORLD,
        config: empty_config(),
        generator: ChunkGeneratorType::Overworld(VanillaGenerator::new(
            BiomeSourceKind::overworld(seed),
            seed,
        )),
        is_flat: false,
        sea_level: sea_level_for_dimension_type(&OVERWORLD),
    })
}

fn create_nether(config: &toml::Value, seed: i64) -> Result<GeneratorOutput, String> {
    validate_empty_config(config)?;
    let seed = seed as u64;
    Ok(GeneratorOutput {
        dimension_type: &THE_NETHER,
        config: empty_config(),
        generator: ChunkGeneratorType::Nether(VanillaGenerator::new(
            BiomeSourceKind::nether(seed),
            seed,
        )),
        is_flat: false,
        sea_level: sea_level_for_dimension_type(&THE_NETHER),
    })
}

fn create_end(config: &toml::Value, seed: i64) -> Result<GeneratorOutput, String> {
    validate_empty_config(config)?;
    let seed = seed as u64;
    Ok(GeneratorOutput {
        dimension_type: &THE_END,
        config: empty_config(),
        generator: ChunkGeneratorType::End(VanillaGenerator::new(BiomeSourceKind::end(seed), seed)),
        is_flat: false,
        sea_level: sea_level_for_dimension_type(&THE_END),
    })
}

fn create_flat(config: &toml::Value, _seed: i64) -> Result<GeneratorOutput, String> {
    let parsed = parse_flat_config(config)?;
    validate_flat_config(config)?;
    let dimension_type = dimension_type_by_key(&parsed.dimension_type)?;
    let normalized_config = normalized_flat_config(&parsed);
    let mut layers = Vec::new();
    for layer in parsed.layers {
        let block = REGISTRY
            .blocks
            .by_key(&layer.block)
            .ok_or_else(|| format!("unknown block {} in minecraft:flat layer", layer.block))?;
        let state = REGISTRY.blocks.get_default_state_id(block);
        layers.extend(repeat_n(state, layer.height));
    }

    Ok(GeneratorOutput {
        dimension_type,
        config: normalized_config,
        generator: ChunkGeneratorType::Flat(FlatChunkGenerator::new_layers(layers)),
        is_flat: true,
        sea_level: sea_level_for_dimension_type(dimension_type),
    })
}

fn create_empty(config: &toml::Value, _seed: i64) -> Result<GeneratorOutput, String> {
    let parsed: DimensionTypeOnlyConfig = config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid steel:empty config: {e}"))?;
    let dimension_type = dimension_type_by_key(&parsed.dimension_type)?;
    Ok(GeneratorOutput {
        dimension_type,
        config: normalized_dimension_type_config(&parsed.dimension_type),
        generator: ChunkGeneratorType::Empty(EmptyChunkGenerator::new()),
        is_flat: false,
        sea_level: sea_level_for_dimension_type(dimension_type),
    })
}

fn empty_config() -> toml::Value {
    toml::Value::Table(Map::new())
}

fn normalized_dimension_type_config(dimension_type: &Identifier) -> toml::Value {
    toml::Value::Table(normalized_dimension_type_table(dimension_type))
}

fn normalized_dimension_type_table(dimension_type: &Identifier) -> Map<String, toml::Value> {
    let mut table = Map::new();
    table.insert(
        "dimension_type".to_owned(),
        toml::Value::String(dimension_type.to_string()),
    );
    table
}

fn normalized_flat_config(config: &FlatGeneratorConfig) -> toml::Value {
    let mut table = normalized_dimension_type_table(&config.dimension_type);
    let layers = config
        .layers
        .iter()
        .map(|layer| {
            let mut layer_table = Map::new();
            layer_table.insert(
                "block".to_owned(),
                toml::Value::String(layer.block.to_string()),
            );
            layer_table.insert(
                "height".to_owned(),
                toml::Value::Integer(layer.height as i64),
            );
            toml::Value::Table(layer_table)
        })
        .collect();
    table.insert("layers".to_owned(), toml::Value::Array(layers));
    toml::Value::Table(table)
}

fn dimension_type_by_key(key: &Identifier) -> Result<DimensionTypeRef, String> {
    REGISTRY
        .dimension_types
        .by_key(key)
        .ok_or_else(|| format!("unknown dimension type {key}"))
}

fn sea_level_for_dimension_type(dimension_type: DimensionTypeRef) -> i32 {
    if dimension_type == &THE_NETHER {
        32
    } else if dimension_type == &THE_END {
        0
    } else {
        63
    }
}
