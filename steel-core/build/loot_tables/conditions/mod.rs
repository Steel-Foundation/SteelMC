use super::{Compiler, TokenStream, model::Holder, quote};
use enum_dispatch::enum_dispatch;
use serde::Deserialize;
mod block;
mod composite;
mod entity;
mod random;

pub use block::{MatchBlock, MatchTool, SurvivesExplosion};
pub use composite::{AllOf, AnyOf, Inverted};
pub use entity::{DamageSourceProperties, EntityProperties, KilledByPlayer, LocationCheck};
pub use random::{RandomChance, RandomChanceWithEnchantedBonus, TableBonus};
impl Compiler {
    pub(super) fn condition(&mut self, condition: Option<&Holder<Condition>>) -> TokenStream {
        match condition {
            None => quote! { true },
            Some(Holder::Reference(key)) => {
                self.with_reference("predicate", key, |compiler, condition| {
                    compiler.condition(Some(condition))
                })
            }
            Some(Holder::Inline(condition)) => condition.compile(self),
        }
    }
}

#[enum_dispatch]
pub(super) trait CompileCondition {
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
