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

use crate::features::{
    BlockStateProviderKind, generate_block_state_provider, generate_block_state_provider_kind_nbt,
};
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
    let provider = parse_provider(required(transform, "block_state_provider"));
    let provider = generate_block_state_provider(&provider);
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
    let update_from_neighbors = transform.get("update_from_neighbors").is_none_or(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("update_from_neighbors must be a boolean"))
    });
    let transform_type = transform_type_token(transform.get("transform_type"));
    let consume_on_use = transform.get("consume_on_use").is_none_or(|value| {
        value
            .as_bool()
            .unwrap_or_else(|| panic!("consume_on_use must be a boolean"))
    });
    let item_damage_per_use = transform.get("item_damage_per_use").map_or(0, |_| {
        let damage = required_i32(transform, "item_damage_per_use");
        assert!(damage >= 0, "item_damage_per_use must be nonnegative");
        damage
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

fn parse_provider(value: &Value) -> BlockStateProviderKind {
    serde_json::from_value(value.clone())
        .unwrap_or_else(|error| panic!("invalid block transformer state provider: {error}"))
}

fn block_transform_nbt_token(value: &Value) -> TokenStream {
    let transform = object(value, "block transformer transform");
    let provider = parse_provider(required(transform, "block_state_provider"));
    let provider = generate_block_state_provider_kind_nbt(&provider);

    let mut fields = vec![quote! { compound.insert("block_state_provider", #provider); }];

    for (name, value) in transform {
        let field = match name.as_str() {
            "block_state_provider" => continue,
            "sound" | "particle" | "loot" | "drop_strategy" | "transform_type" => {
                let value = string(value, name);
                quote! { compound.insert(#name, #value); }
            }

            "disallowed_faces" => {
                let values: Vec<_> = value
                    .as_array()
                    .unwrap_or_else(|| panic!("faces must be an array"))
                    .iter()
                    .map(|value| string(value, name))
                    .collect();
                quote! { compound.insert(#name, NbtList::String(vec![#(#values.into()),*])); }
            }

            "update_from_neighbors" | "consume_on_use" => {
                let value = i8::from(
                    value
                        .as_bool()
                        .unwrap_or_else(|| panic!("{name} must be a boolean")),
                );
                quote! { compound.insert(#name, #value); }
            }

            "item_damage_per_use" => {
                let value = required_i32(transform, name);
                quote! { compound.insert(#name, #value); }
            }
            field => panic!("unsupported block transformer field {field}"),
        };

        fields.push(field);
    }

    quote! {{
        let mut compound = NbtCompound::new();
        #(#fields)*
        compound
    }}
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
        assert!(
            (1..=200).contains(&transforms.len()),
            "block transformer {name} must contain between 1 and 200 transforms"
        );
        entries.push((name, transforms));
    }
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use std::sync::LazyLock;
        use steel_utils::Identifier;
        use crate::block_transformer::{BlockTransformer, BlockTransformerRegistry};
        use crate::{feature::*, vanilla_blocks, vanilla_fluids};
        use steel_utils::value_providers::IntProvider;
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        use glam::IVec3;
    });

    let mut register_stream = TokenStream::new();
    for (name, transforms) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        let key = quote! { Identifier::vanilla_static(#name) };
        let nbt_fn = Ident::new(&format!("{name}_nbt"), Span::call_site());
        let nbt = transforms
            .iter()
            .map(block_transform_nbt_token)
            .collect::<Vec<_>>();
        let transforms = transforms
            .iter()
            .map(block_transform_token)
            .collect::<Vec<_>>();

        stream.extend(quote! {
            fn #nbt_fn() -> NbtList {
                NbtList::Compound(vec![#(#nbt),*])
            }

            pub static #ident: LazyLock<BlockTransformer> = LazyLock::new(|| BlockTransformer {
                key: #key,
                transforms: vec![#(#transforms),*],
                nbt: #nbt_fn,
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
