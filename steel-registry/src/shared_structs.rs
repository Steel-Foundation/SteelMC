use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Deserializer, de::Error as _};
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::Identifier;

/// Block state data as encoded by vanilla registry JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockStateData {
    /// Block identifier.
    #[serde(rename = "Name")]
    pub name: Identifier,
    /// String-valued block-state properties.
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

/// Fluid state data as encoded by vanilla registry JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FluidStateData {
    /// Fluid identifier.
    #[serde(rename = "Name")]
    pub name: Identifier,
    /// String-valued fluid-state properties.
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

pub fn deserialize_tag_identifier<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Identifier, D::Error> {
    let value = String::deserialize(deserializer)?;
    let tag = value.strip_prefix('#').unwrap_or(&value);
    Identifier::from_str(tag).map_err(D::Error::custom)
}

pub fn deserialize_optional_tag_identifier<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Identifier>, D::Error> {
    let Some(value) = Option::<String>::deserialize(deserializer)? else {
        return Ok(None);
    };
    let tag = value.strip_prefix('#').unwrap_or(&value);
    Identifier::from_str(tag)
        .map(Some)
        .map_err(D::Error::custom)
}

/// A single entry in the list of spawn conditions.
#[derive(Debug)]
pub struct SpawnConditionEntry {
    pub priority: i32,
    pub condition: Option<BiomeCondition>,
}

impl ToNbtTag for &SpawnConditionEntry {
    fn to_nbt_tag(self) -> NbtTag {
        let mut e = NbtCompound::new();
        e.insert("priority", self.priority);
        if let Some(cond) = &self.condition {
            e.insert("condition", cond.to_nbt_tag());
        }
        NbtTag::Compound(e)
    }
}

/// Defines a condition based on a biome or list of biomes.
#[derive(Debug)]
pub struct BiomeCondition {
    pub condition_type: &'static str,
    pub biomes: &'static str,
}

impl ToNbtTag for &BiomeCondition {
    fn to_nbt_tag(self) -> NbtTag {
        let mut c = NbtCompound::new();
        c.insert("type", self.condition_type);
        c.insert("biomes", self.biomes);
        NbtTag::Compound(c)
    }
}

/// Serialize a `spawn_conditions` list into the enclosing compound.
/// Matches vanilla's `[{priority, condition?}, …]` shape exactly.
pub fn insert_spawn_conditions(compound: &mut NbtCompound, entries: &[SpawnConditionEntry]) {
    let list: Vec<NbtCompound> = entries
        .iter()
        .map(|entry| {
            let mut e = NbtCompound::new();
            e.insert("priority", entry.priority);
            if let Some(cond) = &entry.condition {
                e.insert("condition", cond.to_nbt_tag());
            }
            e
        })
        .collect();
    compound.insert("spawn_conditions", NbtTag::List(NbtList::Compound(list)));
}
