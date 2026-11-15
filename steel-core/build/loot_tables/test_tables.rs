//! Synthetic plugin tables used only to exercise the compiled-function contract.
use super::{Compiler, TokenStream, identifier, quote};
use serde_json::json;

fn inputs() -> impl IntoIterator<Item = (&'static str, serde_json::Value)> {
    [
        (
            "EMPTY_ALTERNATIVE",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:alternatives","children":[{"type":"minecraft:empty"},{"type":"minecraft:item","name":"minecraft:diamond"}]}]}]}),
        ),
        (
            "GROUP_WEIGHT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:group","children":[{"type":"minecraft:item","name":"minecraft:diamond"},{"type":"minecraft:item","name":"minecraft:iron_ingot"}]},{"type":"minecraft:item","name":"minecraft:dirt","weight":8}]}]}),
        ),
        (
            "SEQUENCE",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:sequence","children":[{"type":"minecraft:item","name":"minecraft:diamond"},{"type":"minecraft:item","name":"minecraft:iron_ingot","condition":{"type":"minecraft:random_chance","chance":0.0}},{"type":"minecraft:item","name":"minecraft:dirt"}]}]}]}),
        ),
        (
            "GROUP_SINGLE_FAILURE",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:alternatives","children":[{"type":"minecraft:group","children":[{"type":"minecraft:item","name":"minecraft:diamond","condition":{"type":"minecraft:random_chance","chance":0.0}}]},{"type":"minecraft:item","name":"minecraft:dirt"}]}]}]}),
        ),
        (
            "MODIFIER_ORDER",
            json!({"modifier":{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","min":1,"max":9},"add":true},"pools":[{"rolls":2,"modifier":{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","min":1,"max":9},"add":true},"entries":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:set_count","count":{"type":"minecraft:uniform","min":2,"max":5}}}]}]}),
        ),
        (
            "TAG_WEIGHT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:tag","items":"#minecraft:creeper_drop_music_discs","expand":true,"weight":2},{"type":"minecraft:item","name":"minecraft:diamond"}]}]}),
        ),
        (
            "UNSUPPORTED_AFTER_OUTPUT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond"}]},{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:iron_ingot","modifier":{"type":"minecraft:enchant_randomly"}}]}]}),
        ),
        (
            "CONDITIONAL_UNSUPPORTED",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:enchant_randomly","condition":{"type":"minecraft:random_chance","chance":0.0}}}]}]}),
        ),
        (
            "OVERSIZED",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:set_count","count":130}}]}]}),
        ),
        (
            "COMPOSITE_MODIFIER",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:group","modifier":{"type":"minecraft:set_count","count":3,"add":true},"children":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:set_count","count":2}}]}]}]}),
        ),
        (
            "PARTIAL_COMPOSITE_MODIFIERS",
            json!({"pools":[{"rolls":1,"entries":[
                {"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:set_count","count":2}},
                {"type":"minecraft:alternatives","modifier":{"type":"minecraft:set_count","count":3,"add":true},"children":[
                    {"type":"minecraft:sequence","modifier":{"type":"minecraft:set_count","count":5,"add":true},"children":[
                        {"type":"minecraft:item","name":"minecraft:iron_ingot","modifier":{"type":"minecraft:set_count","count":7}},
                        {"type":"minecraft:empty","condition":{"type":"minecraft:random_chance","chance":0.0}},
                        {"type":"minecraft:item","name":"minecraft:gold_ingot"}
                    ]},
                    {"type":"minecraft:item","name":"minecraft:dirt","modifier":{"type":"minecraft:set_count","count":11}}
                ]},
                {"type":"minecraft:item","name":"minecraft:copper_ingot","modifier":{"type":"minecraft:set_count","count":13}}
            ]}]}),
        ),
        (
            "RAW_NESTED",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:loot_table","value":{"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:set_count","count":130}}]}]},"modifier":{"type":"minecraft:limit_count","limit":{"max":10}}}]}]}),
        ),
        (
            "INSTRUMENT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:goat_horn","modifier":{"type":"minecraft:set_instrument","options":"#minecraft:regular_goat_horns"}}]}]}),
        ),
        (
            "BOOK_ENCHANTMENT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:book","modifier":[{"type":"minecraft:set_enchantments","enchantments":{"minecraft:sharpness":10}},{"type":"minecraft:set_enchantments","enchantments":{"minecraft:sharpness":-4},"add":true}]}]}]}),
        ),
        (
            "ATTACKER_LOOTING",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","modifier":{"type":"minecraft:enchanted_count_increase","enchantment":"minecraft:looting","count":1.0,"limit":3}}]}]}),
        ),
        (
            "PROVIDER_CHANCE",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","condition":{"type":"minecraft:random_chance","chance":{"type":"minecraft:uniform","min":0.0,"max":1.0}}}]}]}),
        ),
        (
            "ENCHANT_MISSING_COMPONENT",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:loot_table","value":"minecraft:blocks/diamond_ore","modifier":{"type":"minecraft:set_enchantments","enchantments":{"minecraft:looting":{"type":"minecraft:uniform","min":1,"max":9}}}}]}]}),
        ),
    ]
}

pub(super) fn build() -> TokenStream {
    let mut compiler = Compiler::default();
    let mut declarations = Vec::new();
    let mut registrations = Vec::new();
    for (name, table) in inputs() {
        let function = compiler.table(
            &serde_json::from_value(table).unwrap_or_else(|e| panic!("Fixture {name}: {e}")),
        );
        let symbol = super::ident(name);
        let key = identifier(&format!("steel:test/{}", name.to_lowercase()));
        declarations.push(quote! {
            pub(crate) static #symbol: crate::loot::LootTable = crate::loot::LootTable {
                key: #key,
                param_set: steel_utils::Identifier::vanilla_static("generic"),
                random_sequence: None,
            };
        });
        registrations.push(quote! { registry.register(&#symbol, #function); });
    }
    let definitions = compiler.definitions;
    quote! {
        #[cfg(test)]
        pub(super) mod fixtures {
            use super::*;
            #(#declarations)*
            #definitions
            pub(crate) fn register(registry: &mut LootTableBehaviorRegistry) { #(#registrations)* }
        }
    }
}
