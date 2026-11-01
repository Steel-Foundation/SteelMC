use super::super::{
    Compiler, TokenStream,
    model::{
        CompileModifier, Condition, ContextFloatProvider, ContextIntProvider, Holder, HolderSet,
        StewEffect,
    },
};
use serde::Deserialize;
use serde::de::IgnoredAny;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantRandomly {
    #[serde(rename = "options")]
    pub _options: Option<HolderSet>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for EnchantRandomly {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Decode compatibility/additional-cost options and implement seeded
        // enchantment selection, level rolls, and book conversion.
        Compiler::unsupported_modifier("minecraft:enchant_randomly")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnchantWithLevels {
    #[serde(rename = "levels")]
    pub _levels: Option<ContextIntProvider>,
    #[serde(rename = "options")]
    pub _options: Option<HolderSet>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for EnchantWithLevels {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Evaluate the level provider and implement vanilla enchanting with
        // configured options, book conversion, and additional-cost handling.
        Compiler::unsupported_modifier("minecraft:enchant_with_levels")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CopyState {
    #[serde(rename = "block")]
    pub _block: Option<String>,
    #[serde(rename = "properties")]
    pub _properties: Option<Vec<String>>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for CopyState {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Copy the selected context block-state properties into the item's
        // block_state component using the configured block's property definitions.
        Compiler::unsupported_modifier("minecraft:copy_state")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetComponents {
    #[serde(rename = "components")]
    pub _components: Option<IgnoredAny>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetComponents {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Decode typed component patches and apply them with item validation.
        Compiler::unsupported_modifier("minecraft:set_components")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetDamage {
    #[serde(rename = "damage")]
    pub _damage: Option<ContextFloatProvider>,
    #[serde(default, rename = "add")]
    pub _add: bool,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetDamage {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Apply remaining-durability fractions with vanilla additive mode,
        // clamping, rounding, and damageable-item checks.
        Compiler::unsupported_modifier("minecraft:set_damage")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplorationMap {
    #[serde(rename = "destination")]
    pub _destination: Option<String>,
    #[serde(rename = "decoration")]
    pub _decoration: Option<String>,
    #[serde(rename = "zoom")]
    pub _zoom: Option<i32>,
    #[serde(rename = "skip_existing_chunks")]
    pub _skip_existing_chunks: Option<bool>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for ExplorationMap {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Decode destination holder sets and search_radius; locate and claim
        // structures from the loot origin, create saved map data, render its biome
        // preview, and add the configured target decoration and zoom.
        Compiler::unsupported_modifier("minecraft:exploration_map")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetName {
    #[serde(rename = "name")]
    pub _name: Option<IgnoredAny>,
    #[serde(rename = "target")]
    pub _target: Option<String>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetName {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Decode and resolve text against the selected context entity, then
        // apply it to the configured item-name or custom-name component.
        Compiler::unsupported_modifier("minecraft:set_name")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetPotion {
    #[serde(rename = "id")]
    pub _id: Option<String>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetPotion {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Resolve the typed potion and update potion_contents while
        // preserving its other fields.
        Compiler::unsupported_modifier("minecraft:set_potion")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetStewEffect {
    #[serde(rename = "effects")]
    pub _effects: Option<Vec<StewEffect>>,
    pub condition: Option<Holder<Condition>>,
}

impl CompileModifier for SetStewEffect {
    fn condition(&self) -> Option<&Holder<Condition>> {
        self.condition.as_ref()
    }
    fn compile(&self, _compiler: &mut Compiler) -> TokenStream {
        // TODO: Select a stew effect with the loot RNG, evaluate its duration with
        // vanilla instant-effect units, and append it to suspicious-stew effects.
        Compiler::unsupported_modifier("minecraft:set_stew_effect")
    }
}
