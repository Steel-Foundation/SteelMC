//! Typed structure processor-list codec data.

use serde::{Deserialize, Deserializer, de::Error as _};
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::{Identifier, value_providers::IntProvider};

use crate::shared_structs::{
    BlockStateData, deserialize_optional_tag_identifier, deserialize_tag_identifier,
};

/// Codec payload for a structure processor list.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StructureProcessorListData {
    /// Ordered processors.
    pub processors: Vec<StructureProcessorKind>,
}

/// A typed vanilla structure processor.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "processor_type")]
pub enum StructureProcessorKind {
    /// Randomly drops input blocks.
    #[serde(rename = "minecraft:block_rot", alias = "block_rot")]
    BlockRot {
        /// Optional tag restricting which blocks may be dropped.
        #[serde(default, deserialize_with = "deserialize_optional_tag_identifier")]
        rottable_blocks: Option<Identifier>,
        /// Keep probability.
        integrity: f32,
    },
    /// Prevents replacement of protected world blocks.
    #[serde(rename = "minecraft:protected_blocks", alias = "protected_blocks")]
    ProtectedBlocks {
        /// Vanilla field name is `value`; it stores the cannot-replace tag.
        #[serde(rename = "value", deserialize_with = "deserialize_tag_identifier")]
        cannot_replace: Identifier,
    },
    /// Applies the first matching rule.
    #[serde(rename = "minecraft:rule", alias = "rule")]
    Rule { rules: Vec<ProcessorRuleData> },
    /// Ages stone/obsidian structure blocks, used by ruined portals.
    #[serde(rename = "minecraft:block_age", alias = "block_age")]
    BlockAge { mossiness: f32 },
    /// Keeps non-full structure blocks submerged in existing lava.
    #[serde(
        rename = "minecraft:lava_submerged_block",
        alias = "lava_submerged_block"
    )]
    LavaSubmergedBlock,
    /// Replaces jigsaw blocks with their `final_state` block state during placement.
    #[serde(rename = "minecraft:jigsaw_replacement", alias = "jigsaw_replacement")]
    JigsawReplacement,
    /// Replaces stone ruin blocks with blackstone variants.
    #[serde(rename = "minecraft:blackstone_replace", alias = "blackstone_replace")]
    BlackstoneReplace,
    /// Skips placing blocks whose template block type is in the ignore list.
    #[serde(rename = "minecraft:block_ignore", alias = "block_ignore")]
    BlockIgnore { blocks: Vec<BlockStateData> },
    /// Snaps template blocks to a heightmap column plus their template-relative Y.
    #[serde(rename = "minecraft:gravity", alias = "gravity")]
    Gravity {
        #[serde(default = "default_gravity_heightmap")]
        heightmap: StructureProcessorHeightmap,
        #[serde(default)]
        offset: i32,
    },
    /// Delegates to another processor but caps successful modifications.
    #[serde(rename = "minecraft:capped", alias = "capped")]
    Capped {
        delegate: Box<StructureProcessorKind>,
        limit: IntProvider,
    },
}

/// Heightmap types referenced by structure processors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum StructureProcessorHeightmap {
    #[serde(rename = "WORLD_SURFACE")]
    WorldSurface,
    #[serde(rename = "MOTION_BLOCKING")]
    MotionBlocking,
    #[serde(rename = "MOTION_BLOCKING_NO_LEAVES")]
    MotionBlockingNoLeaves,
    #[serde(rename = "OCEAN_FLOOR")]
    OceanFloor,
    #[serde(rename = "WORLD_SURFACE_WG")]
    WorldSurfaceWg,
    #[serde(rename = "OCEAN_FLOOR_WG")]
    OceanFloorWg,
}

const fn default_gravity_heightmap() -> StructureProcessorHeightmap {
    StructureProcessorHeightmap::WorldSurfaceWg
}

/// One rule inside vanilla's `RuleProcessor`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessorRuleData {
    pub input_predicate: StructureRuleTestData,
    pub location_predicate: StructureRuleTestData,
    #[serde(default)]
    pub position_predicate: PosRuleTestData,
    pub output_state: BlockStateData,
    #[serde(default)]
    pub block_entity_modifier: RuleBlockEntityModifierData,
}

/// Block-state rule tests used by `RuleProcessor`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "predicate_type")]
pub enum StructureRuleTestData {
    #[serde(rename = "minecraft:always_true", alias = "always_true")]
    AlwaysTrue,
    #[serde(rename = "minecraft:block_match", alias = "block_match")]
    BlockMatch { block: Identifier },
    #[serde(rename = "minecraft:random_block_match", alias = "random_block_match")]
    RandomBlockMatch { block: Identifier, probability: f32 },
    #[serde(rename = "minecraft:tag_match", alias = "tag_match")]
    TagMatch { tag: Identifier },
    #[serde(rename = "minecraft:blockstate_match", alias = "blockstate_match")]
    BlockStateMatch { block_state: BlockStateData },
    #[serde(
        rename = "minecraft:random_blockstate_match",
        alias = "random_blockstate_match"
    )]
    RandomBlockStateMatch {
        block_state: BlockStateData,
        probability: f32,
    },
}

/// Position rule tests used by `RuleProcessor`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "predicate_type")]
pub enum PosRuleTestData {
    #[default]
    #[serde(rename = "minecraft:always_true", alias = "always_true")]
    AlwaysTrue,
    #[serde(
        rename = "minecraft:axis_aligned_linear_pos",
        alias = "axis_aligned_linear_pos"
    )]
    AxisAlignedLinearPos {
        #[serde(
            default = "default_structure_processor_axis",
            deserialize_with = "deserialize_processor_axis"
        )]
        axis: StructureProcessorAxis,
        #[serde(default)]
        min_chance: f32,
        #[serde(default)]
        max_chance: f32,
        #[serde(default)]
        min_dist: i32,
        #[serde(default)]
        max_dist: i32,
    },
}

/// Axis enum for position predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureProcessorAxis {
    X,
    Y,
    Z,
}

const fn default_structure_processor_axis() -> StructureProcessorAxis {
    StructureProcessorAxis::Y
}

fn deserialize_processor_axis<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<StructureProcessorAxis, D::Error> {
    let value = String::deserialize(deserializer)?;
    match value.as_str() {
        "x" => Ok(StructureProcessorAxis::X),
        "y" => Ok(StructureProcessorAxis::Y),
        "z" => Ok(StructureProcessorAxis::Z),
        _ => Err(D::Error::custom("invalid structure processor axis")),
    }
}

/// Rule block-entity NBT modifiers.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "type")]
pub enum RuleBlockEntityModifierData {
    /// Vanilla passthrough when the field is absent.
    #[default]
    Passthrough,
    /// Appends loot table metadata to the output block entity.
    #[serde(rename = "minecraft:append_loot")]
    AppendLoot { loot_table: Identifier },
    /// Merges static NBT into the output block entity.
    #[serde(rename = "minecraft:append_static")]
    AppendStatic {
        #[serde(deserialize_with = "deserialize_static_nbt_compound")]
        data: NbtCompound,
    },
}

fn deserialize_static_nbt_compound<'de, D>(deserializer: D) -> Result<NbtCompound, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    json_value_to_nbt_compound(&value).map_err(D::Error::custom)
}

fn json_value_to_nbt_compound(value: &serde_json::Value) -> Result<NbtCompound, String> {
    let serde_json::Value::Object(object) = value else {
        return Err("append_static data must be an object".to_owned());
    };

    let mut compound = NbtCompound::new();
    for (key, value) in object {
        compound.insert(key.as_str(), json_value_to_nbt_tag(value)?);
    }
    Ok(compound)
}

fn json_value_to_nbt_tag(value: &serde_json::Value) -> Result<NbtTag, String> {
    match value {
        serde_json::Value::Null => Err("null is not a valid NBT tag".to_owned()),
        serde_json::Value::Bool(value) => Ok(NbtTag::Byte(i8::from(*value))),
        serde_json::Value::Number(value) => json_number_to_nbt_tag(value),
        serde_json::Value::String(value) => Ok(NbtTag::String(value.clone().into())),
        serde_json::Value::Array(values) => {
            let values = values
                .iter()
                .map(json_value_to_nbt_tag)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(NbtTag::List(NbtList::from(values)))
        }
        serde_json::Value::Object(_) => Ok(NbtTag::Compound(json_value_to_nbt_compound(value)?)),
    }
}

fn json_number_to_nbt_tag(value: &serde_json::Number) -> Result<NbtTag, String> {
    if let Some(value) = value.as_i64() {
        return i32::try_from(value)
            .map(NbtTag::Int)
            .or_else(|_| Ok(NbtTag::Long(value)));
    }

    if let Some(value) = value.as_u64() {
        return i32::try_from(value)
            .map(NbtTag::Int)
            .or_else(|_| i64::try_from(value).map(NbtTag::Long))
            .map_err(|_| format!("NBT integer value {value} does not fit i64"));
    }

    value
        .as_f64()
        .map(NbtTag::Double)
        .ok_or_else(|| format!("invalid NBT numeric value {value}"))
}
