//! Runtime registry for world generator factories.

use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use std::iter::repeat_n;
use steel_registry::dimension_type::DimensionTypeRef;
use steel_registry::vanilla_biomes;
use steel_registry::vanilla_dimension_types::{OVERWORLD, THE_END, THE_NETHER};
use steel_registry::{REGISTRY, RegistryExt};
use steel_utils::Identifier;
use toml::map::Map;

use crate::worldgen::structure::{FixedStructureBiomeProvider, StructureGenerator};
use crate::worldgen::{
    ChunkGeneratorType, EmptyChunkGenerator, FlatChunkGenerator, VanillaGenerator,
};
use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::structure::placement::load_vanilla_structure_sets;

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
    validate: fn(&toml::Value) -> Result<WorldGeneratorConfigData, String>,
    create: fn(&WorldGeneratorConfigData, i64) -> Result<GeneratorOutput, String>,
}

/// Generator config after parsing, validation, and default application.
#[derive(Debug, Clone)]
pub struct ValidatedWorldGeneratorConfig {
    generator: Identifier,
    data: WorldGeneratorConfigData,
}

impl ValidatedWorldGeneratorConfig {
    /// Generator factory identifier this config was validated for.
    #[must_use]
    pub const fn generator(&self) -> &Identifier {
        &self.generator
    }

    /// Dimension type selected by this validated generator config.
    #[must_use]
    pub fn dimension_type(&self) -> DimensionTypeRef {
        match &self.data {
            WorldGeneratorConfigData::Empty => fixed_generator_dimension_type(&self.generator),
            WorldGeneratorConfigData::EmptyWorld(config) => {
                validated_dimension_type_by_key(&config.dimension_type)
            }
            WorldGeneratorConfigData::Flat(config) => {
                validated_dimension_type_by_key(&config.dimension_type)
            }
        }
    }
}

#[derive(Debug, Clone)]
enum WorldGeneratorConfigData {
    Empty,
    EmptyWorld(DimensionTypeOnlyConfig),
    Flat(FlatGeneratorConfig),
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
            Identifier::from_steel("empty"),
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

    /// Parses and validates config for a generator ID.
    pub fn validate_config(
        &self,
        key: &Identifier,
        config: &toml::Value,
    ) -> Result<ValidatedWorldGeneratorConfig, String> {
        let factory = self
            .factories
            .get(key)
            .ok_or_else(|| format!("unknown world generator {key}"))?;
        let data = (factory.validate)(config)?;
        Ok(ValidatedWorldGeneratorConfig {
            generator: key.clone(),
            data,
        })
    }

    /// Creates a generator from a validated generator ID and config.
    pub fn create(
        &self,
        config: &ValidatedWorldGeneratorConfig,
        seed: i64,
    ) -> Result<GeneratorOutput, String> {
        let factory = self
            .factories
            .get(&config.generator)
            .ok_or_else(|| format!("unknown world generator {}", config.generator))?;
        (factory.create)(&config.data, seed)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DimensionTypeOnlyConfig {
    dimension_type: Identifier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlatGeneratorConfig {
    #[serde(default = "default_flat_dimension_type")]
    dimension_type: Identifier,
    #[serde(default = "default_flat_layers")]
    layers: Vec<FlatLayerConfig>,
    #[serde(default)]
    features: bool,
    #[serde(default)]
    lakes: bool,
    #[serde(default = "default_flat_structure_overrides")]
    structure_overrides: Vec<Identifier>,
}

#[derive(Debug, Clone, Deserialize)]
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

fn default_flat_structure_overrides() -> Vec<Identifier> {
    vec![
        Identifier::vanilla_static("strongholds"),
        Identifier::vanilla_static("villages"),
    ]
}

fn validate_empty_config(config: &toml::Value) -> Result<WorldGeneratorConfigData, String> {
    let Some(table) = config.as_table() else {
        return Err("generator config must be a table".to_owned());
    };
    if !table.is_empty() {
        return Err("this generator does not accept config".to_owned());
    }
    Ok(WorldGeneratorConfigData::Empty)
}

fn validate_empty_world_config(config: &toml::Value) -> Result<WorldGeneratorConfigData, String> {
    let parsed: DimensionTypeOnlyConfig = config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid steel:empty config: {e}"))?;
    dimension_type_by_key(&parsed.dimension_type)?;
    Ok(WorldGeneratorConfigData::EmptyWorld(parsed))
}

fn validate_flat_config(config: &toml::Value) -> Result<WorldGeneratorConfigData, String> {
    let parsed = parse_flat_config(config)?;
    if parsed.layers.is_empty() {
        return Err("minecraft:flat requires at least one layer".to_owned());
    }
    // TODO: Implement vanilla FlatLevelGeneratorSettings::adjustGenerationSettings for these flags.
    if parsed.features {
        return Err("minecraft:flat features=true is not implemented yet".to_owned());
    }
    if parsed.lakes {
        return Err("minecraft:flat lakes=true is not implemented yet".to_owned());
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
    let available_structure_sets: FxHashSet<_> = load_vanilla_structure_sets()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    for structure_set in &parsed.structure_overrides {
        if !available_structure_sets.contains(structure_set) {
            return Err(format!(
                "unknown structure set {structure_set} in minecraft:flat structure_overrides"
            ));
        }
    }
    Ok(WorldGeneratorConfigData::Flat(parsed))
}

fn parse_flat_config(config: &toml::Value) -> Result<FlatGeneratorConfig, String> {
    config
        .clone()
        .try_into()
        .map_err(|e| format!("invalid minecraft:flat config: {e}"))
}

fn create_overworld(
    config: &WorldGeneratorConfigData,
    seed: i64,
) -> Result<GeneratorOutput, String> {
    let WorldGeneratorConfigData::Empty = config else {
        return Err("validated config does not match minecraft:overworld".to_owned());
    };
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

fn create_nether(config: &WorldGeneratorConfigData, seed: i64) -> Result<GeneratorOutput, String> {
    let WorldGeneratorConfigData::Empty = config else {
        return Err("validated config does not match minecraft:the_nether".to_owned());
    };
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

fn create_end(config: &WorldGeneratorConfigData, seed: i64) -> Result<GeneratorOutput, String> {
    let WorldGeneratorConfigData::Empty = config else {
        return Err("validated config does not match minecraft:the_end".to_owned());
    };
    let seed = seed as u64;
    Ok(GeneratorOutput {
        dimension_type: &THE_END,
        config: empty_config(),
        generator: ChunkGeneratorType::End(VanillaGenerator::new(BiomeSourceKind::end(seed), seed)),
        is_flat: false,
        sea_level: sea_level_for_dimension_type(&THE_END),
    })
}

fn create_flat(config: &WorldGeneratorConfigData, seed: i64) -> Result<GeneratorOutput, String> {
    let WorldGeneratorConfigData::Flat(parsed) = config else {
        return Err("validated config does not match minecraft:flat".to_owned());
    };
    let dimension_type = dimension_type_by_key(&parsed.dimension_type)?;
    let normalized_config = normalized_flat_config(parsed);
    let mut layers = Vec::new();
    for layer in &parsed.layers {
        let block = REGISTRY
            .blocks
            .by_key(&layer.block)
            .ok_or_else(|| format!("unknown block {} in minecraft:flat layer", layer.block))?;
        let state = REGISTRY.blocks.get_default_state_id(block);
        layers.extend(repeat_n(state, layer.height));
    }

    let structure_generator = if parsed.structure_overrides.is_empty() {
        None
    } else {
        let structure_sets = load_vanilla_structure_sets()
            .into_iter()
            .filter(|(key, _)| parsed.structure_overrides.contains(key))
            .collect();
        let biome_provider = FixedStructureBiomeProvider::new(&vanilla_biomes::PLAINS);
        Some(StructureGenerator::vanilla_flat_with_structure_sets(
            seed,
            &biome_provider,
            structure_sets,
        ))
    };

    Ok(GeneratorOutput {
        dimension_type,
        config: normalized_config,
        generator: ChunkGeneratorType::Flat(FlatChunkGenerator::new_layers_with_structures(
            layers,
            seed,
            sea_level_for_dimension_type(dimension_type),
            structure_generator,
        )),
        is_flat: true,
        sea_level: sea_level_for_dimension_type(dimension_type),
    })
}

fn create_empty(config: &WorldGeneratorConfigData, _seed: i64) -> Result<GeneratorOutput, String> {
    let WorldGeneratorConfigData::EmptyWorld(parsed) = config else {
        return Err("validated config does not match steel:empty".to_owned());
    };
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
    table.insert("features".to_owned(), toml::Value::Boolean(config.features));
    table.insert("lakes".to_owned(), toml::Value::Boolean(config.lakes));
    table.insert(
        "structure_overrides".to_owned(),
        toml::Value::Array(
            config
                .structure_overrides
                .iter()
                .map(|key| toml::Value::String(key.to_string()))
                .collect(),
        ),
    );
    toml::Value::Table(table)
}

fn dimension_type_by_key(key: &Identifier) -> Result<DimensionTypeRef, String> {
    REGISTRY
        .dimension_types
        .by_key(key)
        .ok_or_else(|| format!("unknown dimension type {key}"))
}

fn validated_dimension_type_by_key(key: &Identifier) -> DimensionTypeRef {
    match dimension_type_by_key(key) {
        Ok(dimension_type) => dimension_type,
        Err(error) => panic!("validated generator config should have a dimension type: {error}"),
    }
}

fn fixed_generator_dimension_type(generator: &Identifier) -> DimensionTypeRef {
    if generator == &Identifier::vanilla_static("overworld") {
        &OVERWORLD
    } else if generator == &Identifier::vanilla_static("the_nether") {
        &THE_NETHER
    } else if generator == &Identifier::vanilla_static("the_end") {
        &THE_END
    } else {
        panic!("validated empty config does not have a fixed dimension type for {generator}")
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::test_support::init_test_registry;

    #[test]
    fn default_flat_config_matches_vanilla_superflat() {
        init_test_registry();

        let registry = WorldGeneratorRegistry::new_with_builtins()
            .expect("built-in generator registry should initialize");
        let config = registry
            .validate_config(
                &Identifier::vanilla_static("flat"),
                &toml::Value::Table(Map::new()),
            )
            .expect("default flat config should validate");
        let output = registry
            .create(&config, 0)
            .expect("default flat config should create a generator");
        let config = output
            .config
            .as_table()
            .expect("normalized flat config should be a table");

        assert_eq!(
            config.get("dimension_type"),
            Some(&toml::Value::String("minecraft:overworld".to_owned()))
        );
        assert_eq!(config.get("features"), Some(&toml::Value::Boolean(false)));
        assert_eq!(config.get("lakes"), Some(&toml::Value::Boolean(false)));
        assert_eq!(
            config.get("structure_overrides"),
            Some(&toml::Value::Array(vec![
                toml::Value::String("minecraft:strongholds".to_owned()),
                toml::Value::String("minecraft:villages".to_owned()),
            ]))
        );
    }

    #[test]
    fn rejects_unimplemented_flat_decoration_options() {
        let features_config = toml::Value::Table(Map::from_iter([(
            "features".to_owned(),
            toml::Value::Boolean(true),
        )]));
        let features_error = validate_flat_config(&features_config)
            .expect_err("features=true should not use non-vanilla decoration");
        assert!(features_error.contains("features=true"));

        let lakes_config = toml::Value::Table(Map::from_iter([(
            "lakes".to_owned(),
            toml::Value::Boolean(true),
        )]));
        let lakes_error = validate_flat_config(&lakes_config)
            .expect_err("lakes=true should not use non-vanilla decoration");
        assert!(lakes_error.contains("lakes=true"));
    }
}
