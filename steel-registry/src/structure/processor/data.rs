//! Typed structure processor-list codec data.

use serde::{Deserialize, Deserializer, de::Error as _};
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
    #[serde(rename = "minecraft:block_rot")]
    BlockRot {
        /// Optional tag restricting which blocks may be dropped.
        #[serde(default, deserialize_with = "deserialize_optional_tag_identifier")]
        rottable_blocks: Option<Identifier>,
        /// Keep probability.
        integrity: f32,
    },
    /// Prevents replacement of protected world blocks.
    #[serde(rename = "minecraft:protected_blocks")]
    ProtectedBlocks {
        /// Vanilla field name is `value`; it stores the cannot-replace tag.
        #[serde(rename = "value", deserialize_with = "deserialize_tag_identifier")]
        cannot_replace: Identifier,
    },
    /// Applies the first matching rule.
    #[serde(rename = "minecraft:rule")]
    Rule { rules: Vec<ProcessorRuleData> },
    /// Ages stone/obsidian structure blocks, used by ruined portals.
    #[serde(rename = "minecraft:block_age")]
    BlockAge { mossiness: f32 },
    /// Keeps non-full structure blocks submerged in existing lava.
    #[serde(rename = "minecraft:lava_submerged_block")]
    LavaSubmergedBlock,
    /// Replaces stone ruin blocks with blackstone variants.
    #[serde(rename = "minecraft:blackstone_replace")]
    BlackstoneReplace,
    /// Delegates to another processor but caps successful modifications.
    #[serde(rename = "minecraft:capped")]
    Capped {
        delegate: Box<StructureProcessorKind>,
        limit: IntProvider,
    },
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
    #[serde(rename = "minecraft:always_true")]
    AlwaysTrue,
    #[serde(rename = "minecraft:block_match")]
    BlockMatch { block: Identifier },
    #[serde(rename = "minecraft:random_block_match")]
    RandomBlockMatch { block: Identifier, probability: f32 },
    #[serde(rename = "minecraft:tag_match")]
    TagMatch { tag: Identifier },
    #[serde(rename = "minecraft:blockstate_match")]
    BlockStateMatch { block_state: BlockStateData },
}

/// Position rule tests used by `RuleProcessor`.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(tag = "predicate_type")]
pub enum PosRuleTestData {
    #[default]
    #[serde(rename = "minecraft:always_true")]
    AlwaysTrue,
    #[serde(rename = "minecraft:axis_aligned_linear_pos")]
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
}
