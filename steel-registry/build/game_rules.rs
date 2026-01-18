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

        let (value_type, default_value) = match rule.value_type.as_str() {
            "bool" => {
                let value = rule.default.as_bool().unwrap_or(false);
                (
                    quote! { GameRuleType::Bool },
                    quote! { GameRuleValue::Bool(#value) },
                )
            }
            "int" => {
                let value = rule.default.as_i64().unwrap_or(0) as i32;
                (
                    quote! { GameRuleType::Int },
                    quote! { GameRuleValue::Int(#value) },
                )
            }
            _ => panic!("Unknown game rule type: {}", rule.value_type),
        };

        let min_value = match rule.min {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };

        let max_value = match rule.max {
            Some(v) => quote! { Some(#v) },
            None => quote! { None },
        };

        constants.extend(quote! {
            pub static #const_name: &GameRule = &GameRule {
                key: Identifier::vanilla_static(#rule_name),
                category: GameRuleCategory::#category,
                value_type: #value_type,
                default_value: #default_value,
                min_value: #min_value,
                max_value: #max_value,
            };
        });

        registrations.extend(quote! {
            registry.register(#const_name);
        });
    }

    quote! {
        use crate::game_rules::{GameRule, GameRuleRegistry, GameRuleCategory, GameRuleType, GameRuleValue};
        use steel_utils::Identifier;

        #constants

        pub fn register_game_rules(registry: &mut GameRuleRegistry) {
            #registrations
        }
    }
}
