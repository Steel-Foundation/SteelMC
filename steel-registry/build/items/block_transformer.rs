use proc_macro2::TokenStream;
use quote::quote;
use serde_json::Value;

use super::{Ident, Span, ToShoutySnakeCase, identifier_token, sound_event_value_token};

pub(super) fn block_transformer_component_token(value: &Value) -> TokenStream {
    let transforms = value
        .as_array()
        .unwrap_or_else(|| panic!("block_transformer component must be an array"))
        .iter()
        .map(block_transform_token)
        .collect::<Vec<_>>();
    assert!(
        !transforms.is_empty(),
        "block_transformer component must contain at least one transform"
    );

    let transformer = path("BlockTransformer");
    quote! {
        #transformer {
            transforms: vec![#(#transforms),*],
        }
    }
}

pub(super) fn pottery_pattern_component_token(value: &Value) -> TokenStream {
    let pattern = value
        .as_str()
        .unwrap_or_else(|| panic!("provides_pottery_pattern component must be an identifier"));
    let (namespace, path) = pattern.split_once(':').unwrap_or(("minecraft", pattern));
    assert_eq!(
        namespace, "minecraft",
        "vanilla provides_pottery_pattern must reference a vanilla pattern: {pattern}"
    );
    let pattern = Ident::new(&path.to_shouty_snake_case(), Span::call_site());

    quote! {
        vanilla_components::ProvidesPotteryPattern {
            pattern: &crate::vanilla_decorated_pot_patterns::#pattern,
        }
    }
}

fn block_transform_token(value: &Value) -> TokenStream {
    let transform = object(value, "block transformer transform");
    assert_known_fields(
        transform,
        &[
            "block_state_provider",
            "sound",
            "particle",
            "disallowed_faces",
            "loot",
            "drop_strategy",
            "transform_type",
            "consume_on_use",
            "item_damage_per_use",
        ],
        "block transformer transform",
    );
    let provider = provider_token(required(transform, "block_state_provider"));
    let sound = sound_event_value_token(required(transform, "sound"), "block_transformer.sound");
    let particle = particle_token(transform.get("particle"));
    let disallowed_faces = transform
        .get("disallowed_faces")
        .map_or_else(Vec::new, |value| {
            value
                .as_array()
                .unwrap_or_else(|| panic!("block_transformer.disallowed_faces must be an array"))
                .iter()
                .map(|face| direction_token(string(face, "block_transformer disallowed face")))
                .collect()
        });
    let loot = transform.get("loot").map_or_else(
        || quote! { None },
        |value| {
            let loot = identifier_token(string(value, "block_transformer.loot"));
            quote! { Some(#loot) }
        },
    );
    let drop_strategy = drop_strategy_token(transform.get("drop_strategy"));
    let transform_type = transform_type_token(transform.get("transform_type"));
    let consume_on_use = transform.get("consume_on_use").map_or(true, |value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("block_transformer.consume_on_use must be a boolean"))
    });
    let item_damage_per_use = transform.get("item_damage_per_use").map_or(1, |value| {
        let value = value
            .as_i64()
            .unwrap_or_else(|| panic!("block_transformer.item_damage_per_use must be an integer"));
        i32::try_from(value).unwrap_or_else(|_| {
            panic!("block_transformer.item_damage_per_use must fit an i32: {value}")
        })
    });

    let block_transform = path("BlockTransformData");
    quote! {
        #block_transform {
            block_state_provider: #provider,
            sound: #sound,
            particle: #particle,
            disallowed_faces: vec![#(#disallowed_faces),*],
            loot: #loot,
            drop_strategy: #drop_strategy,
            transform_type: #transform_type,
            consume_on_use: #consume_on_use,
            item_damage_per_use: #item_damage_per_use,
        }
    }
}

fn provider_token(value: &Value) -> TokenStream {
    let provider = object(value, "block transformer state provider");
    let provider_type = string(
        required(provider, "type"),
        "block transformer state provider type",
    );
    let provider = path("TransformStateProvider");
    match provider_type {
        "minecraft:simple_state_provider" => {
            assert_known_fields(
                object(value, "simple state provider"),
                &["type", "state"],
                "simple state provider",
            );
            let state =
                block_state_token(required(object(value, "simple state provider"), "state"));
            quote! { #provider::Simple { state: #state } }
        }
        "minecraft:copy_properties_provider" => {
            assert_known_fields(
                object(value, "copy properties provider"),
                &["type", "source_block_state_provider"],
                "copy properties provider",
            );
            let source = provider_token(required(
                object(value, "copy properties provider"),
                "source_block_state_provider",
            ));
            quote! { #provider::CopyProperties { source: Box::new(#source) } }
        }
        "minecraft:rule_based_state_provider" => {
            let provider_value = object(value, "rule based state provider");
            assert_known_fields(
                provider_value,
                &["type", "fallback", "rules"],
                "rule based state provider",
            );
            let fallback = provider_value.get("fallback").map_or_else(
                || quote! { None },
                |value| {
                    let provider = provider_token(value);
                    quote! { Some(Box::new(#provider)) }
                },
            );
            let rules = required(provider_value, "rules")
                .as_array()
                .unwrap_or_else(|| panic!("rule based state provider rules must be an array"))
                .iter()
                .map(rule_token)
                .collect::<Vec<_>>();
            quote! {
                #provider::RuleBased {
                    fallback: #fallback,
                    rules: vec![#(#rules),*],
                }
            }
        }
        unsupported => {
            panic!("unsupported extracted block transformer state provider {unsupported}")
        }
    }
}

fn rule_token(value: &Value) -> TokenStream {
    let rule = object(value, "rule based state provider rule");
    assert_known_fields(rule, &["if_true", "then"], "rule based state provider rule");
    let if_true = predicate_token(required(rule, "if_true"));
    let then = provider_token(required(rule, "then"));
    let rule = path("TransformStateProviderRule");
    quote! { #rule { if_true: #if_true, then: #then } }
}

fn predicate_token(value: &Value) -> TokenStream {
    let predicate = object(value, "block transformer predicate");
    let predicate_type = string(
        required(predicate, "type"),
        "block transformer predicate type",
    );
    let predicate_path = path("TransformPredicate");
    match predicate_type {
        "minecraft:matching_blocks" => {
            assert_known_fields(
                predicate,
                &["type", "offset", "blocks"],
                "matching blocks predicate",
            );
            let offset = offset_token(predicate.get("offset"));
            let blocks = holder_set_token(required(predicate, "blocks"), "matching_blocks.blocks");
            quote! { #predicate_path::MatchingBlocks { offset: #offset, blocks: #blocks } }
        }
        "minecraft:matching_block_tag" => {
            assert_known_fields(
                predicate,
                &["type", "offset", "tag"],
                "matching block tag predicate",
            );
            let offset = offset_token(predicate.get("offset"));
            let tag =
                identifier_token(string(required(predicate, "tag"), "matching_block_tag.tag"));
            quote! { #predicate_path::MatchingBlockTag { offset: #offset, tag: #tag } }
        }
        "minecraft:all_of" => {
            assert_known_fields(predicate, &["type", "predicates"], "all_of predicate");
            let predicates = required(predicate, "predicates")
                .as_array()
                .unwrap_or_else(|| panic!("all_of.predicates must be an array"))
                .iter()
                .map(predicate_token)
                .collect::<Vec<_>>();
            quote! { #predicate_path::All(vec![#(#predicates),*]) }
        }
        unsupported => panic!("unsupported extracted block transformer predicate {unsupported}"),
    }
}

fn block_state_token(value: &Value) -> TokenStream {
    let state = object(value, "block transformer block state");
    assert_known_fields(
        state,
        &["Name", "Properties"],
        "block transformer block state",
    );
    let block = identifier_token(string(
        required(state, "Name"),
        "block transformer block Name",
    ));
    let properties = state.get("Properties").map_or_else(Vec::new, |value| {
        let properties = object(value, "block transformer block Properties");
        properties
            .iter()
            .map(|(name, value)| {
                let value = string(value, "block transformer block property");
                quote! { (#name.to_owned(), #value.to_owned()) }
            })
            .collect()
    });
    let state = path("TransformBlockState");
    quote! {
        #state {
            block: #block,
            properties: vec![#(#properties),*],
        }
    }
}

fn holder_set_token(value: &Value, field: &str) -> TokenStream {
    let holder_set = path("TransformHolderSet");
    if let Some(value) = value.as_str() {
        let entry = identifier_token(value);
        return quote! { #holder_set::Entries(vec![#entry]) };
    }
    let values = value
        .as_array()
        .unwrap_or_else(|| panic!("{field} must be an identifier or identifier array"))
        .iter()
        .map(|value| identifier_token(string(value, field)))
        .collect::<Vec<_>>();
    assert!(!values.is_empty(), "{field} must not be empty");
    quote! { #holder_set::Entries(vec![#(#values),*]) }
}

fn offset_token(value: Option<&Value>) -> TokenStream {
    let offset = value.map_or([0, 0, 0], |value| {
        let values = value
            .as_array()
            .unwrap_or_else(|| panic!("block transformer predicate offset must be an array"));
        assert_eq!(
            values.len(),
            3,
            "block transformer predicate offset must have three coordinates"
        );
        [0, 1, 2].map(|index| {
            let value = &values[index];
            let value = value.as_i64().unwrap_or_else(|| {
                panic!("block transformer predicate offset must contain integers")
            });
            i32::try_from(value).unwrap_or_else(|_| {
                panic!("block transformer predicate offset value must fit an i32: {value}")
            })
        })
    });
    let [x, y, z] = offset;
    quote! { (#x, #y, #z) }
}

fn particle_token(value: Option<&Value>) -> TokenStream {
    let particle = path("TransformParticle");
    match value.map_or("none", |value| string(value, "block_transformer.particle")) {
        "none" => quote! { #particle::None },
        "scrape" => quote! { #particle::Scrape },
        "wax_on" => quote! { #particle::WaxOn },
        "wax_off" => quote! { #particle::WaxOff },
        value => panic!("unsupported block_transformer particle {value}"),
    }
}

fn drop_strategy_token(value: Option<&Value>) -> TokenStream {
    let drop_strategy = path("DropStrategy");
    match value.map_or("from_middle", |value| {
        string(value, "block_transformer.drop_strategy")
    }) {
        "from_middle" => quote! { #drop_strategy::FromMiddle },
        "clicked_face" => quote! { #drop_strategy::ClickedFace },
        value => panic!("unsupported block_transformer drop_strategy {value}"),
    }
}

fn transform_type_token(value: Option<&Value>) -> TokenStream {
    let transform_type = path("TransformType");
    match value.map_or("single_block", |value| {
        string(value, "block_transformer.transform_type")
    }) {
        "single_block" => quote! { #transform_type::SingleBlock },
        "copper_chest" => quote! { #transform_type::CopperChest },
        value => panic!("unsupported block_transformer transform_type {value}"),
    }
}

fn direction_token(value: &str) -> TokenStream {
    let direction = match value {
        "down" => quote! { steel_utils::Direction::Down },
        "up" => quote! { steel_utils::Direction::Up },
        "north" => quote! { steel_utils::Direction::North },
        "south" => quote! { steel_utils::Direction::South },
        "west" => quote! { steel_utils::Direction::West },
        "east" => quote! { steel_utils::Direction::East },
        value => panic!("unsupported block_transformer direction {value}"),
    };
    direction
}

fn path(type_name: &str) -> TokenStream {
    let type_name = Ident::new(type_name, Span::call_site());
    quote! { crate::data_components::components::block_transformer::#type_name }
}

fn object<'a>(value: &'a Value, field: &str) -> &'a serde_json::Map<String, Value> {
    value
        .as_object()
        .unwrap_or_else(|| panic!("{field} must be an object"))
}

fn required<'a>(object: &'a serde_json::Map<String, Value>, field: &str) -> &'a Value {
    object
        .get(field)
        .unwrap_or_else(|| panic!("missing required field {field}"))
}

fn string<'a>(value: &'a Value, field: &str) -> &'a str {
    value
        .as_str()
        .unwrap_or_else(|| panic!("{field} must be a string"))
}

fn assert_known_fields(object: &serde_json::Map<String, Value>, allowed: &[&str], context: &str) {
    for key in object.keys() {
        assert!(
            allowed.contains(&key.as_str()),
            "{context} contains unsupported field {key}"
        );
    }
}
