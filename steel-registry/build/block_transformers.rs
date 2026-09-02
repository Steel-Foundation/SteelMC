//! Builds the `minecraft:block_transformer` dynamic registry from datapack
//! JSON. Vanilla moved this from inline per-item component data to a proper
//! registry (`Registries.BLOCK_TRANSFORMER`); items now just reference an
//! entry by identifier (see `build/items/block_transformer.rs`).
//!
//! Type ids and field names below were verified against this version's
//! decompiled `BlockStateProviderTypes`/`BlockPredicateType`/`BlockTransformer`
//! sources, not carried over from an older schema.

use std::fs;
use std::str::FromStr;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde_json::Value;
use steel_utils::Identifier;

use crate::generator_functions::generate_sound_event_ref;

fn path(type_name: &str) -> TokenStream {
    let type_name = Ident::new(type_name, Span::call_site());
    quote! { crate::block_transformer::#type_name }
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

fn identifier_token(value: &str) -> TokenStream {
    let (namespace, path) = value.split_once(':').unwrap_or(("minecraft", value));
    quote! { Identifier::new_static(#namespace, #path) }
}

fn block_transform_token(value: &Value) -> TokenStream {
    let transform = object(value, "block transformer transform");
    let provider = provider_token(required(transform, "block_state_provider"));
    let sound = sound_token(transform.get("sound"));
    let particle = particle_token(transform.get("particle"));
    let disallowed_faces = transform
        .get("disallowed_faces")
        .map_or_else(Vec::new, |value| {
            value
                .as_array()
                .unwrap_or_else(|| panic!("disallowed_faces must be an array"))
                .iter()
                .map(|face| direction_token(string(face, "disallowed face")))
                .collect()
        });
    let loot = transform.get("loot").map_or_else(
        || quote! { None },
        |value| {
            let loot = identifier_token(string(value, "loot"));
            quote! { Some(#loot) }
        },
    );
    let drop_strategy = drop_strategy_token(transform.get("drop_strategy"));
    let update_from_neighbors = transform
        .get("update_from_neighbors")
        .map_or(true, |value| {
            value
                .as_bool()
                .unwrap_or_else(|| panic!("update_from_neighbors must be a boolean"))
        });
    let transform_type = transform_type_token(transform.get("transform_type"));
    let consume_on_use = transform.get("consume_on_use").map_or(true, |value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("consume_on_use must be a boolean"))
    });
    let item_damage_per_use = transform.get("item_damage_per_use").map_or(0, |value| {
        let value = value
            .as_i64()
            .unwrap_or_else(|| panic!("item_damage_per_use must be an integer"));
        i32::try_from(value)
            .unwrap_or_else(|_| panic!("item_damage_per_use must fit an i32: {value}"))
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
            update_from_neighbors: #update_from_neighbors,
            transform_type: #transform_type,
            consume_on_use: #consume_on_use,
            item_damage_per_use: #item_damage_per_use,
        }
    }
}

fn sound_token(value: Option<&Value>) -> TokenStream {
    let sound_holder = quote! { crate::sound_event::SoundEventHolder };
    let Some(value) = value else {
        // Vanilla's `SoundEvents.EMPTY`.
        return quote! {
            #sound_holder::Direct { sound_id: Identifier::vanilla_static("empty"), fixed_range: None }
        };
    };
    let sound = string(value, "sound");
    let id = Identifier::from_str(sound)
        .unwrap_or_else(|error| panic!("invalid sound event id {sound:?}: {error}"));
    let sound_ref = generate_sound_event_ref(&id);
    quote! { #sound_holder::Registry(#sound_ref) }
}

/// `BlockStateProvider.CODEC`: `Either<BlockState, TypedProvider>`. A value
/// without `type` is a bare block state (implicit `SimpleStateProvider`).
fn provider_token(value: &Value) -> TokenStream {
    let object_value = object(value, "block state provider");
    let provider = path("TransformStateProvider");
    let Some(provider_type) = object_value.get("type") else {
        let state = block_state_token(value);
        return quote! { #provider::Simple { state: #state } };
    };
    let provider_type = string(provider_type, "block state provider type");
    match provider_type {
        "minecraft:simple" => {
            let state = block_state_token(required(object_value, "state"));
            quote! { #provider::Simple { state: #state } }
        }
        "minecraft:copy_properties" => {
            let source = provider_token(required(object_value, "source"));
            quote! { #provider::CopyProperties { source: Box::new(#source) } }
        }
        "minecraft:rotated" => {
            let state = provider_token(required(object_value, "state"));
            let direction = object_value.get("direction").map_or_else(
                || quote! { None },
                |value| {
                    let direction = direction_token(string(value, "rotated provider direction"));
                    quote! { Some(#direction) }
                },
            );
            quote! { #provider::RotatedBlock { state: Box::new(#state), direction: #direction } }
        }
        "minecraft:weighted" => {
            let entries = required(object_value, "entries")
                .as_array()
                .unwrap_or_else(|| panic!("weighted provider entries must be an array"))
                .iter()
                .map(weighted_state_token)
                .collect::<Vec<_>>();
            assert!(
                !entries.is_empty(),
                "weighted provider entries must not be empty"
            );
            quote! { #provider::Weighted { entries: vec![#(#entries),*] } }
        }
        "minecraft:randomized_int" => {
            let source = provider_token(required(object_value, "source"));
            let property = string(
                required(object_value, "property"),
                "randomized_int property",
            );
            let values = int_provider_token(required(object_value, "values"));
            quote! {
                #provider::RandomizedInt {
                    source: Box::new(#source),
                    property: #property.to_owned(),
                    values: #values,
                }
            }
        }
        "minecraft:noise" => {
            let (seed, noise, scale) = noise_fields_token(object_value);
            let states = block_state_list_token(required(object_value, "states"));
            quote! { #provider::Noise { seed: #seed, noise: #noise, scale: #scale, states: vec![#(#states),*] } }
        }
        "minecraft:noise_threshold" => {
            let (seed, noise, scale) = noise_fields_token(object_value);
            let threshold = required_f32(object_value, "threshold");
            let high_chance = required_f32(object_value, "high_chance");
            let default_state = block_state_token(required(object_value, "default_state"));
            let low_states = block_state_list_token(required(object_value, "low_states"));
            let high_states = block_state_list_token(required(object_value, "high_states"));
            quote! {
                #provider::NoiseThreshold {
                    seed: #seed,
                    noise: #noise,
                    scale: #scale,
                    threshold: #threshold,
                    high_chance: #high_chance,
                    default_state: #default_state,
                    low_states: vec![#(#low_states),*],
                    high_states: vec![#(#high_states),*],
                }
            }
        }
        "minecraft:dual_noise" => {
            let (seed, noise, scale) = noise_fields_token(object_value);
            let variety = required(object_value, "variety")
                .as_array()
                .unwrap_or_else(|| panic!("dual_noise variety must be an array"));
            assert_eq!(variety.len(), 2, "dual_noise variety must have two entries");
            let min = i32::try_from(
                variety[0]
                    .as_i64()
                    .expect("variety entries must be integers"),
            )
            .expect("variety entry must fit an i32");
            let max = i32::try_from(
                variety[1]
                    .as_i64()
                    .expect("variety entries must be integers"),
            )
            .expect("variety entry must fit an i32");
            let slow_noise = noise_parameters_token(required(object_value, "slow_noise"));
            let slow_scale = required_f32(object_value, "slow_scale");
            let states = block_state_list_token(required(object_value, "states"));
            quote! {
                #provider::DualNoise {
                    variety: (#min, #max),
                    slow_noise: #slow_noise,
                    slow_scale: #slow_scale,
                    seed: #seed,
                    noise: #noise,
                    scale: #scale,
                    states: vec![#(#states),*],
                }
            }
        }
        "minecraft:rule_based" => {
            let fallback = object_value.get("fallback").map_or_else(
                || quote! { None },
                |value| {
                    let provider = provider_token(value);
                    quote! { Some(Box::new(#provider)) }
                },
            );
            let rules = required(object_value, "rules")
                .as_array()
                .unwrap_or_else(|| panic!("rule based provider rules must be an array"))
                .iter()
                .map(rule_token)
                .collect::<Vec<_>>();
            quote! { #provider::RuleBased { fallback: #fallback, rules: vec![#(#rules),*] } }
        }
        unsupported => panic!("unsupported block transformer state provider type {unsupported}"),
    }
}

fn weighted_state_token(value: &Value) -> TokenStream {
    let entry = object(value, "weighted state provider entry");
    let data = block_state_token(required(entry, "data"));
    let weight = required(entry, "weight")
        .as_i64()
        .unwrap_or_else(|| panic!("weighted state provider entry weight must be an integer"));
    let weight = i32::try_from(weight).unwrap_or_else(|_| panic!("weight must fit an i32"));
    let weighted_state = path("WeightedTransformBlockState");
    quote! { #weighted_state { data: #data, weight: #weight } }
}

fn noise_fields_token(
    provider: &serde_json::Map<String, Value>,
) -> (TokenStream, TokenStream, TokenStream) {
    let seed = required(provider, "seed")
        .as_i64()
        .unwrap_or_else(|| panic!("noise provider seed must be an integer"));
    let noise = noise_parameters_token(required(provider, "noise"));
    let scale = required_f32(provider, "scale");
    (quote! { #seed }, noise, quote! { #scale })
}

fn noise_parameters_token(value: &Value) -> TokenStream {
    let parameters = object(value, "noise parameters");
    let first_octave = required(parameters, "firstOctave")
        .as_i64()
        .unwrap_or_else(|| panic!("noise parameters firstOctave must be an integer"));
    let first_octave =
        i32::try_from(first_octave).unwrap_or_else(|_| panic!("firstOctave must fit an i32"));
    let amplitudes = parameters
        .get("amplitudes")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|value| {
                    value
                        .as_f64()
                        .unwrap_or_else(|| panic!("amplitude must be a number"))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let noise_parameters = path("TransformNoiseParameters");
    quote! {
        #noise_parameters { first_octave: #first_octave, amplitudes: vec![#(#amplitudes),*] }
    }
}

fn int_provider_token(value: &Value) -> TokenStream {
    let int_provider = path("IntProviderLiteral");
    if let Some(value) = value.as_i64() {
        let value = i32::try_from(value).unwrap_or_else(|_| panic!("int provider must fit an i32"));
        return quote! { #int_provider::Constant(#value) };
    }
    let provider = object(value, "int provider");
    let provider_type = string(required(provider, "type"), "int provider type");
    match provider_type {
        "minecraft:constant" => {
            let value = required(provider, "value")
                .as_i64()
                .unwrap_or_else(|| panic!("constant int provider value must be an integer"));
            let value = i32::try_from(value).unwrap_or_else(|_| panic!("value must fit an i32"));
            quote! { #int_provider::Constant(#value) }
        }
        "minecraft:uniform" => {
            let min = required_i32(provider, "min_inclusive");
            let max = required_i32(provider, "max_inclusive");
            quote! { #int_provider::Uniform { min_inclusive: #min, max_inclusive: #max } }
        }
        unsupported => panic!("unsupported block transformer int provider type {unsupported}"),
    }
}

fn rule_token(value: &Value) -> TokenStream {
    let rule = object(value, "rule based provider rule");
    let if_true = predicate_token(required(rule, "if_true"));
    let then = provider_token(required(rule, "then"));
    let rule_type = path("TransformStateProviderRule");
    quote! { #rule_type { if_true: #if_true, then: #then } }
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
            let offset = offset_token(predicate.get("offset"));
            let blocks = holder_set_token(required(predicate, "blocks"), "matching_blocks.blocks");
            quote! { #predicate_path::MatchingBlocks { offset: #offset, blocks: #blocks } }
        }
        "minecraft:matching_block_tag" => {
            let offset = offset_token(predicate.get("offset"));
            let tag =
                identifier_token(string(required(predicate, "tag"), "matching_block_tag.tag"));
            quote! { #predicate_path::MatchingBlockTag { offset: #offset, tag: #tag } }
        }
        "minecraft:matching_fluids" => {
            let offset = offset_token(predicate.get("offset"));
            let fluids = holder_set_token(required(predicate, "fluids"), "matching_fluids.fluids");
            quote! { #predicate_path::MatchingFluids { offset: #offset, fluids: #fluids } }
        }
        "minecraft:matching_biomes" => {
            let biomes = holder_set_token(required(predicate, "biomes"), "matching_biomes.biomes");
            quote! { #predicate_path::MatchingBiomes { biomes: #biomes } }
        }
        "minecraft:has_sturdy_face" => {
            let offset = offset_token(predicate.get("offset"));
            let direction = direction_token(string(
                required(predicate, "direction"),
                "has_sturdy_face.direction",
            ));
            quote! { #predicate_path::HasSturdyFace { offset: #offset, direction: #direction } }
        }
        "minecraft:solid" => {
            let offset = offset_token(predicate.get("offset"));
            quote! { #predicate_path::Solid { offset: #offset } }
        }
        "minecraft:replaceable" => {
            let offset = offset_token(predicate.get("offset"));
            quote! { #predicate_path::Replaceable { offset: #offset } }
        }
        "minecraft:would_survive" => {
            let offset = offset_token(predicate.get("offset"));
            let state = block_state_token(required(predicate, "state"));
            quote! { #predicate_path::WouldSurvive { offset: #offset, state: #state } }
        }
        "minecraft:inside_world_bounds" => {
            let offset = offset_token(predicate.get("offset"));
            quote! { #predicate_path::InsideWorldBounds { offset: #offset } }
        }
        "minecraft:any_of" => {
            let predicates = required(predicate, "predicates")
                .as_array()
                .unwrap_or_else(|| panic!("any_of predicates must be an array"))
                .iter()
                .map(predicate_token)
                .collect::<Vec<_>>();
            quote! { #predicate_path::AnyOf(vec![#(#predicates),*]) }
        }
        "minecraft:all_of" => {
            let predicates = required(predicate, "predicates")
                .as_array()
                .unwrap_or_else(|| panic!("all_of predicates must be an array"))
                .iter()
                .map(predicate_token)
                .collect::<Vec<_>>();
            quote! { #predicate_path::AllOf(vec![#(#predicates),*]) }
        }
        "minecraft:not" => {
            let inner = predicate_token(required(predicate, "predicate"));
            quote! { #predicate_path::Not(Box::new(#inner)) }
        }
        "minecraft:true" => quote! { #predicate_path::True },
        "minecraft:unobstructed" => {
            let offset = offset_token(predicate.get("offset"));
            quote! { #predicate_path::Unobstructed { offset: #offset } }
        }
        unsupported => panic!("unsupported block transformer predicate type {unsupported}"),
    }
}

fn block_state_list_token(value: &Value) -> Vec<TokenStream> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("block state list must be an array"))
        .iter()
        .map(block_state_token)
        .collect()
}

/// `StateHolder.codec()` field tags: lowercase `id`/`properties`.
fn block_state_token(value: &Value) -> TokenStream {
    let state = object(value, "block state");
    let block = identifier_token(string(required(state, "id"), "block state id"));
    let properties = state.get("properties").map_or_else(Vec::new, |value| {
        object(value, "block state properties")
            .iter()
            .map(|(name, value)| {
                let value = string(value, "block state property value");
                quote! { (#name.to_owned(), #value.to_owned()) }
            })
            .collect()
    });
    let state_type = path("TransformBlockState");
    quote! {
        #state_type {
            block: #block,
            properties: vec![#(#properties),*],
        }
    }
}

fn holder_set_token(value: &Value, field: &str) -> TokenStream {
    let holder_set = path("TransformHolderSet");
    if let Some(value) = value.as_str() {
        if let Some(tag) = value.strip_prefix('#') {
            let tag = identifier_token(tag);
            return quote! { #holder_set::Tag(#tag) };
        }
        let entry = identifier_token(value);
        return quote! { #holder_set::Entries(vec![#entry]) };
    }
    let values = value
        .as_array()
        .unwrap_or_else(|| panic!("{field} must be an identifier, tag, or identifier array"))
        .iter()
        .map(|value| identifier_token(string(value, field)))
        .collect::<Vec<_>>();
    quote! { #holder_set::Entries(vec![#(#values),*]) }
}

fn offset_token(value: Option<&Value>) -> TokenStream {
    let offset = value.map_or([0, 0, 0], |value| {
        let values = value
            .as_array()
            .unwrap_or_else(|| panic!("predicate offset must be an array"));
        assert_eq!(
            values.len(),
            3,
            "predicate offset must have three coordinates"
        );
        [0, 1, 2].map(|index| {
            let value = values[index]
                .as_i64()
                .unwrap_or_else(|| panic!("predicate offset must contain integers"));
            i32::try_from(value).unwrap_or_else(|_| panic!("offset value must fit an i32: {value}"))
        })
    });
    let [x, y, z] = offset;
    quote! { (#x, #y, #z) }
}

fn particle_token(value: Option<&Value>) -> TokenStream {
    let particle = path("TransformParticle");
    match value.map_or("none", |value| string(value, "particle")) {
        "none" => quote! { #particle::None },
        "scrape" => quote! { #particle::Scrape },
        "wax_on" => quote! { #particle::WaxOn },
        "wax_off" => quote! { #particle::WaxOff },
        value => panic!("unsupported block transformer particle {value}"),
    }
}

fn drop_strategy_token(value: Option<&Value>) -> TokenStream {
    let drop_strategy = path("DropStrategy");
    match value.map_or("from_middle", |value| string(value, "drop_strategy")) {
        "from_middle" => quote! { #drop_strategy::FromMiddle },
        "clicked_face" => quote! { #drop_strategy::ClickedFace },
        value => panic!("unsupported block transformer drop_strategy {value}"),
    }
}

fn transform_type_token(value: Option<&Value>) -> TokenStream {
    let transform_type = path("TransformType");
    match value.map_or("single_block", |value| string(value, "transform_type")) {
        "single_block" => quote! { #transform_type::SingleBlock },
        "copper_chest" => quote! { #transform_type::CopperChest },
        value => panic!("unsupported block transformer transform_type {value}"),
    }
}

fn direction_token(value: &str) -> TokenStream {
    match value {
        "down" => quote! { steel_utils::Direction::Down },
        "up" => quote! { steel_utils::Direction::Up },
        "north" => quote! { steel_utils::Direction::North },
        "south" => quote! { steel_utils::Direction::South },
        "west" => quote! { steel_utils::Direction::West },
        "east" => quote! { steel_utils::Direction::East },
        value => panic!("unsupported block transformer direction {value}"),
    }
}

fn required_f32(object: &serde_json::Map<String, Value>, field: &str) -> f32 {
    required(object, field)
        .as_f64()
        .unwrap_or_else(|| panic!("{field} must be a number")) as f32
}

fn required_i32(object: &serde_json::Map<String, Value>, field: &str) -> i32 {
    let value = required(object, field)
        .as_i64()
        .unwrap_or_else(|| panic!("{field} must be an integer"));
    i32::try_from(value).unwrap_or_else(|_| panic!("{field} must fit an i32"))
}

pub(crate) fn build() -> TokenStream {
    let dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/block_transformer";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let content = fs::read_to_string(&path).unwrap();
        let transforms: Vec<Value> = serde_json::from_str(&content)
            .unwrap_or_else(|error| panic!("failed to parse {name}: {error}"));
        entries.push((name, transforms));
    }
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use std::sync::LazyLock;
        use steel_utils::Identifier;
        use crate::block_transformer::{BlockTransformer, BlockTransformerRegistry};
    });

    let mut register_stream = TokenStream::new();
    for (name, transforms) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let transforms = transforms
            .iter()
            .map(block_transform_token)
            .collect::<Vec<_>>();

        stream.extend(quote! {
            pub static #ident: LazyLock<BlockTransformer> = LazyLock::new(|| BlockTransformer {
                key: #key,
                transforms: vec![#(#transforms),*],
            });
        });
        register_stream.extend(quote! {
            registry.register(&*#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_block_transformers(registry: &mut BlockTransformerRegistry) {
            #register_stream
        }
    });

    stream
}
