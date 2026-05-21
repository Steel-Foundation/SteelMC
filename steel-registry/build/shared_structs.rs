use std::{collections::BTreeMap, str::FromStr};

use serde::{Deserialize, Deserializer, de::Error as _};
use steel_utils::Identifier;

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct BlockStateData {
    #[serde(rename = "Name")]
    pub name: Identifier,
    #[serde(rename = "Properties", default)]
    pub properties: BTreeMap<String, String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
pub struct FluidStateData {
    #[serde(rename = "Name")]
    pub name: Identifier,
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

#[derive(Deserialize, Debug)]
pub struct SpawnConditionEntry {
    pub(crate) priority: i32,
    #[serde(default)]
    pub(crate) condition: Option<BiomeCondition>,
}

#[derive(Deserialize, Debug)]
pub struct BiomeCondition {
    #[serde(rename = "type")]
    pub(crate) condition_type: String,
    pub(crate) biomes: String,
}

#[derive(Deserialize, Debug)]
pub struct TextComponentJson {
    pub(crate) translate: String,
}
