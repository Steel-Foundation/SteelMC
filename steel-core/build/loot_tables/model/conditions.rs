pub use super::super::conditions::{
    AllOf, AnyOf, DamageSourceProperties, EntityProperties, Inverted, KilledByPlayer,
    LocationCheck, MatchBlock, MatchTool, RandomChance, RandomChanceWithEnchantedBonus,
    SurvivesExplosion, TableBonus,
};
use super::super::{Compiler, TokenStream};

use enum_dispatch::enum_dispatch;
use serde::Deserialize;

#[enum_dispatch]
pub(in super::super) trait CompileCondition {
    fn compile(&self, compiler: &mut Compiler) -> TokenStream;
}

// TODO: Add vanilla conditions: enchantment_active_check, entity_scores,
// environment_attribute_check, float_value_check, int_value_check, time_check,
// and weather_check.
#[enum_dispatch(CompileCondition)]
#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Condition {
    #[serde(rename = "minecraft:survives_explosion")]
    SurvivesExplosion(SurvivesExplosion),
    #[serde(rename = "minecraft:killed_by_player")]
    KilledByPlayer(KilledByPlayer),
    #[serde(rename = "minecraft:match_block")]
    MatchBlock(MatchBlock),
    #[serde(rename = "minecraft:match_tool")]
    MatchTool(MatchTool),
    #[serde(rename = "minecraft:random_chance")]
    RandomChance(RandomChance),
    #[serde(rename = "minecraft:table_bonus")]
    TableBonus(TableBonus),
    #[serde(rename = "minecraft:random_chance_with_enchanted_bonus")]
    RandomChanceWithEnchantedBonus(RandomChanceWithEnchantedBonus),
    #[serde(rename = "minecraft:inverted")]
    Inverted(Inverted),
    #[serde(rename = "minecraft:any_of")]
    AnyOf(AnyOf),
    #[serde(rename = "minecraft:all_of")]
    AllOf(AllOf),
    #[serde(rename = "minecraft:entity_properties")]
    EntityProperties(EntityProperties),
    #[serde(rename = "minecraft:damage_source_properties")]
    DamageSourceProperties(DamageSourceProperties),
    // Location evaluation is explicitly unsupported; its predicate payload is
    // consumed without claiming to validate or implement that separate codec.
    #[serde(rename = "minecraft:location_check")]
    LocationCheck(LocationCheck),
}

#[derive(Deserialize)]
pub enum EntityTarget {
    // TODO: Add target_entity and use vanilla's attacking_player input name.
    #[serde(rename = "this")]
    This,
    #[serde(rename = "attacker")]
    Attacker,
    #[serde(rename = "direct_attacker")]
    DirectAttacker,
    #[serde(rename = "last_damage_player")]
    LastDamagePlayer,
    #[serde(rename = "interacting_entity")]
    InteractingEntity,
}
