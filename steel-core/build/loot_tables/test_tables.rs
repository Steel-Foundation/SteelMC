//! Synthetic plugin tables used only to exercise the compiled-function contract.
use super::{Compiler, TokenStream, identifier, model::LootTable, quote};
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
            "POTION",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:tipped_arrow","modifier":{"type":"minecraft:set_potion","id":"minecraft:strong_slowness"}}]}]}),
        ),
        (
            "DAMAGE",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:stone_pickaxe","modifier":{"type":"minecraft:set_damage","damage":0.25}}]},{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:stone","modifier":{"type":"minecraft:set_damage","damage":{"type":"minecraft:uniform","min":0.0,"max":1.0}}}]}]}),
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
        (
            "FISHING_HOOK_PREDICATES",
            json!({"pools":[
                {"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:stone","condition":{"type":"minecraft:entity_properties","entity":"this","predicate":{"minecraft:type_specific/fishing_hook":{}}}}]},
                {"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:gold_ingot","condition":{"type":"minecraft:entity_properties","entity":"this","predicate":{"minecraft:type_specific/fishing_hook":{"in_open_water":false}}}}]},
                {"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:diamond","condition":{"type":"minecraft:entity_properties","entity":"this","predicate":{"minecraft:type_specific/fishing_hook":{"in_open_water":true}}}}]}
            ]}),
        ),
    ]
}

fn exploration_inputs() -> impl IntoIterator<Item = (&'static str, serde_json::Value)> {
    [
        (
            "EXPLORATION_DIRECT_DESTINATION",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:map","modifier":{"type":"minecraft:exploration_map","destination":"minecraft:buried_treasure"}}]}]}),
        ),
        (
            "EXPLORATION_DESTINATION_LIST",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:map","modifier":{"type":"minecraft:exploration_map","destination":["minecraft:buried_treasure","minecraft:monument"]}}]}]}),
        ),
        (
            "EXPLORATION_EMPTY_DESTINATION_LIST",
            json!({"pools":[{"rolls":1,"entries":[{"type":"minecraft:item","name":"minecraft:map","modifier":{"type":"minecraft:exploration_map","destination":[]}}]}]}),
        ),
    ]
}

pub(super) fn build(vanilla_tables: &[(String, LootTable)]) -> TokenStream {
    let mut compiler = Compiler::default();
    let mut resumable = Compiler {
        resumable: true,
        ..Compiler::default()
    };
    let mut declarations = Vec::new();
    let mut registrations = Vec::new();
    let mut async_registrations = Vec::new();
    for (name, table) in inputs()
        .into_iter()
        .chain(exploration_inputs())
        .chain(super::location_test_tables::inputs())
        .chain(super::name_test_tables::inputs())
    {
        let table = serde_json::from_value(table).unwrap_or_else(|e| panic!("Fixture {name}: {e}"));
        let function = (!super::requirements::requires_resumption(&table, vanilla_tables))
            .then(|| compiler.table(&table));
        let async_function = resumable.table(&table);
        let symbol = super::ident(name);
        let key = identifier(&format!("steel:test/{}", name.to_lowercase()));
        declarations.push(quote! {
            pub(crate) static #symbol: crate::loot::LootTable = crate::loot::LootTable {
                key: #key,
                param_set: steel_utils::Identifier::vanilla_static("generic"),
                random_sequence: None,
            };
        });
        if let Some(function) = function {
            registrations.push(quote! { registry.register_pair(&#symbol, #function, super::resumable::#async_function); });
        } else {
            async_registrations
                .push(quote! { registry.register_async(&#symbol, #async_function); });
        }
    }
    let definitions = compiler.definitions;
    let async_definitions = resumable.definitions;
    quote! {
        #[cfg(test)]
        pub(super) mod fixtures {
            use super::{LootContext, ItemStack, vanilla_items, REGISTRY, TaggedRegistryExt, LootContextEntity, vanilla_loot_tables, LootTableBehaviorRegistry};
            #(#declarations)*
            mod direct {
                use super::*;
                use crate::loot::{LootCandidate, LootOutput, OperationResult, with_modifier};
                const fn identity(_: &mut LootContext<'_>, _: &mut ItemStack) -> OperationResult<()> { Ok(()) }
                #definitions
                pub(super) fn register(registry: &mut LootTableBehaviorRegistry) { #(#registrations)* }
            }
            mod resumable {
                use super::*;
                use crate::loot::{AsyncLootCandidate as LootCandidate, LootAsyncModifier as LootModifier,
                    LootAsyncOutput as LootOutput, ModifiedAsyncLootOutput as ModifiedLootOutput, LootFuture};
                fn identity<'a>(_: &'a mut LootContext<'_>, _: &'a mut ItemStack) -> LootFuture<'a> { Box::pin(async { Ok(()) }) }
                #async_definitions
                pub(super) fn register(registry: &mut LootTableBehaviorRegistry) { #(#async_registrations)* }
            }
            pub(crate) fn register(registry: &mut LootTableBehaviorRegistry) {
                direct::register(registry);
                resumable::register(registry);
            }
        }
    }
}
