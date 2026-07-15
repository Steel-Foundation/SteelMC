use std::fs;

use heck::{ToPascalCase, ToShoutySnakeCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize)]
struct GameRulesFile {
    game_rules: Vec<GameRuleEntry>,
}

#[derive(Deserialize)]
struct GameRuleEntry {
    name: String,
    category: String,
    #[serde(rename = "type")]
    value_type: String,
    default: serde_json::Value,
    min: Option<i32>,
    max: Option<i32>,
}

pub fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/game_rules.json");

    let json_content =
        fs::read_to_string("build_assets/game_rules.json").expect("Failed to read game_rules.json");
    let game_rules_file: GameRulesFile =
        serde_json::from_str(&json_content).expect("Failed to parse game_rules.json");

    let mut constants = TokenStream::new();
    let mut registrations = TokenStream::new();

    for rule in &game_rules_file.game_rules {
        let const_name = Ident::new(&rule.name.to_shouty_snake_case(), Span::call_site());
        let rule_name = Literal::string(&rule.name);
        let category = Ident::new(&rule.category.to_pascal_case(), Span::call_site());

        let (value_type, definition) = match rule.value_type.as_str() {
            "bool" => {
                let Some(value) = rule.default.as_bool() else {
                    panic!("Boolean game rule {} has a non-boolean default", rule.name);
                };
                (
                    quote! { bool },
                    quote! {
                        GameRule::boolean(
                            Identifier::vanilla_static(#rule_name),
                            GameRuleCategory::#category,
                            #value,
                        )
                    },
                )
            }
            "int" => {
                let Some(value) = rule.default.as_i64() else {
                    panic!("Integer game rule {} has a non-integer default", rule.name);
                };
                let Ok(value) = i32::try_from(value) else {
                    panic!("Integer game rule {} default is outside i32", rule.name);
                };
                let min_value = if let Some(v) = rule.min {
                    quote! { Some(#v) }
                } else {
                    quote! { None }
                };
                let max_value = if let Some(v) = rule.max {
                    quote! { Some(#v) }
                } else {
                    quote! { None }
                };
                (
                    quote! { i32 },
                    quote! {
                        GameRule::integer(
                            Identifier::vanilla_static(#rule_name),
                            GameRuleCategory::#category,
                            #value,
                            #min_value,
                            #max_value,
                        )
                    },
                )
            }
            _ => panic!("Unknown game rule type: {}", rule.value_type),
        };

        constants.extend(quote! {
            pub static #const_name: GameRule<#value_type> = #definition;
        });

        registrations.extend(quote! {
            registry.register(&#const_name);
        });
    }

    quote! {
        use crate::game_rules::{GameRule, GameRuleRegistry, GameRuleCategory};
        use steel_utils::Identifier;

        #constants

        pub fn register_game_rules(registry: &mut GameRuleRegistry) {
            #registrations
        }
    }
}
