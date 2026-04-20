use serde::Deserialize;

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
