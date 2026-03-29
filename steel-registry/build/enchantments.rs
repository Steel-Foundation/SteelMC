use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct EnchantmentJson {
    max_level: u32,
    min_cost: CostJson,
    max_cost: CostJson,
    anvil_cost: i32,
    weight: u32,
    slots: Vec<String>,
    supported_items: String,
    primary_items: Option<String>,
    exclusive_set: Option<String>,
}

#[derive(Deserialize, Debug)]
struct CostJson {
    base: i32,
    per_level_above_first: i32,
}

fn slot_to_tokens(slot: &str) -> TokenStream {
    match slot {
        "any" => quote! { EquipmentSlotGroup::Any },
        "hand" => quote! { EquipmentSlotGroup::Hand },
        "mainhand" => quote! { EquipmentSlotGroup::MainHand },
        "offhand" => quote! { EquipmentSlotGroup::OffHand },
        "armor" => quote! { EquipmentSlotGroup::Armor },
        "head" => quote! { EquipmentSlotGroup::Head },
        "chest" => quote! { EquipmentSlotGroup::Chest },
        "legs" => quote! { EquipmentSlotGroup::Legs },
        "feet" => quote! { EquipmentSlotGroup::Feet },
        "body" => quote! { EquipmentSlotGroup::Body },
        other => panic!("Unknown equipment slot group: {other}"),
    }
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/enchantment/"
    );

    let enchantment_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/enchantment";
    let mut enchantments = Vec::new();

    for entry in fs::read_dir(enchantment_dir).expect("Failed to read enchantment directory") {
        let entry = entry.expect("Failed to read directory entry");
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }

        let name = path
            .file_stem()
            .expect("No file stem")
            .to_str()
            .expect("Invalid UTF-8")
            .to_string();
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let ench: EnchantmentJson = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {name}: {e}"));

        enchantments.push((name, ench));
    }

    enchantments.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::enchantment::{Enchantment, EnchantmentCost, EnchantmentRegistry};
        use crate::loot_table::EquipmentSlotGroup;
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();

    for (name, ench) in &enchantments {
        let const_ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());

        let max_level = Literal::u32_unsuffixed(ench.max_level);
        let min_cost_base = Literal::i32_unsuffixed(ench.min_cost.base);
        let min_cost_per = Literal::i32_unsuffixed(ench.min_cost.per_level_above_first);
        let max_cost_base = Literal::i32_unsuffixed(ench.max_cost.base);
        let max_cost_per = Literal::i32_unsuffixed(ench.max_cost.per_level_above_first);
        let anvil_cost = Literal::i32_unsuffixed(ench.anvil_cost);
        let weight = Literal::u32_unsuffixed(ench.weight);

        let slots: Vec<TokenStream> = ench.slots.iter().map(|s| slot_to_tokens(s)).collect();

        let supported_items = ench.supported_items.as_str();
        let primary_items = match &ench.primary_items {
            Some(s) => {
                let s = s.as_str();
                quote! { Some(#s) }
            }
            None => quote! { None },
        };
        let exclusive_set = match &ench.exclusive_set {
            Some(s) => {
                let s = s.as_str();
                quote! { Some(#s) }
            }
            None => quote! { None },
        };

        stream.extend(quote! {
            pub static #const_ident: Enchantment = Enchantment {
                key: Identifier::vanilla_static(#name),
                max_level: #max_level,
                min_cost: EnchantmentCost { base: #min_cost_base, per_level_above_first: #min_cost_per },
                max_cost: EnchantmentCost { base: #max_cost_base, per_level_above_first: #max_cost_per },
                anvil_cost: #anvil_cost,
                weight: #weight,
                slots: &[#(#slots),*],
                supported_items: #supported_items,
                primary_items: #primary_items,
                exclusive_set: #exclusive_set,
            };
        });

        register_stream.extend(quote! {
            registry.register(&#const_ident);
        });
    }

    stream.extend(quote! {
        pub fn register_enchantments(registry: &mut EnchantmentRegistry) {
            #register_stream
        }
    });

    stream
}
