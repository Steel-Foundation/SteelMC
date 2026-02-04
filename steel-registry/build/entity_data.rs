//! Build script for generating entity data structs from entities.json.
//!
//! Generates a struct per entity type with SyncedValue fields for each
//! synched_data entry, along with `new()`, `pack_dirty()`, and `pack_all()` methods.

use std::fs;

use heck::{ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
struct EntityEntry {
    #[allow(dead_code)]
    id: i32,
    name: String,
    synched_data: Vec<SynchedDataEntry>,
}

#[derive(Deserialize, Debug)]
struct SynchedDataEntry {
    index: u8,
    name: String,
    serializer: String,
    #[serde(default)]
    default_value: Value,
}

/// Maps a serializer name to (Rust type, EntityData variant, serializer ID).
fn serializer_info(serializer: &str) -> Option<(&'static str, &'static str, i32)> {
    // Serializer IDs must match the registration order in vanilla_serializers.rs
    Some(match serializer {
        "byte" => ("i8", "Byte", 0),
        "int" => ("i32", "Int", 1),
        "long" => ("i64", "Long", 2),
        "float" => ("f32", "Float", 3),
        "string" => ("String", "String", 4),
        "component" => ("Box<TextComponent>", "Component", 5),
        "optional_component" => ("Option<Box<TextComponent>>", "OptionalComponent", 6),
        "item_stack" => ("ItemStack", "ItemStack", 7),
        "boolean" => ("bool", "Boolean", 8),
        "rotations" => ("Rotations", "Rotations", 9),
        "block_pos" => ("BlockPos", "BlockPos", 10),
        "optional_block_pos" => ("Option<BlockPos>", "OptionalBlockPos", 11),
        "direction" => ("Direction", "Direction", 12),
        "optional_living_entity_reference" => ("Option<Uuid>", "OptionalLivingEntityRef", 13),
        "block_state" => ("BlockStateId", "BlockState", 14),
        "optional_block_state" => ("Option<BlockStateId>", "OptionalBlockState", 15),
        "particle" => ("ParticleData", "Particle", 16),
        "particles" => ("ParticleList", "Particles", 17),
        "villager_data" => ("VillagerData", "VillagerData", 18),
        "optional_unsigned_int" => ("Option<u32>", "OptionalUnsignedInt", 19),
        "pose" => ("EntityPose", "Pose", 20),
        "cat_variant" => ("i32", "CatVariant", 21),
        "cow_variant" => ("i32", "CowVariant", 22),
        "wolf_variant" => ("i32", "WolfVariant", 23),
        "wolf_sound_variant" => ("i32", "WolfSoundVariant", 24),
        "frog_variant" => ("i32", "FrogVariant", 25),
        "pig_variant" => ("i32", "PigVariant", 26),
        "chicken_variant" => ("i32", "ChickenVariant", 27),
        "zombie_nautilus_variant" => ("i32", "ZombieNautilusVariant", 28),
        "optional_global_pos" => ("Option<GlobalPos>", "OptionalGlobalPos", 29),
        "painting_variant" => ("i32", "PaintingVariant", 30),
        "sniffer_state" => ("SnifferState", "SnifferState", 31),
        "armadillo_state" => ("ArmadilloState", "ArmadilloState", 32),
        "copper_golem_state" => ("i32", "CopperGolemState", 33),
        "weathering_copper_state" => ("i32", "WeatheringCopperState", 34),
        "vector3" => ("Vector3f", "Vector3", 35),
        "quaternion" => ("Quaternionf", "Quaternion", 36),
        "resolvable_profile" => ("ResolvableProfile", "ResolvableProfile", 37),
        "humanoid_arm" => ("HumanoidArm", "HumanoidArm", 38),
        _ => return None,
    })
}

/// Generate the default value expression for a field.
fn default_value_expr(serializer: &str, default: &Value) -> TokenStream {
    match serializer {
        "byte" => {
            let v = default.as_i64().unwrap_or(0) as i8;
            quote! { #v }
        }
        "int" => {
            let v = default.as_i64().unwrap_or(0) as i32;
            quote! { #v }
        }
        "long" => {
            let v = default.as_i64().unwrap_or(0);
            quote! { #v }
        }
        "float" => {
            let v = default.as_f64().unwrap_or(0.0) as f32;
            let lit = Literal::f32_suffixed(v);
            quote! { #lit }
        }
        "string" => {
            let v = default.as_str().unwrap_or("");
            quote! { #v.to_string() }
        }
        "boolean" => {
            let v = default.as_bool().unwrap_or(false);
            quote! { #v }
        }
        "optional_component"
        | "optional_block_pos"
        | "optional_block_state"
        | "optional_living_entity_reference"
        | "optional_unsigned_int"
        | "optional_global_pos" => {
            quote! { None }
        }
        "pose" => {
            let pose_str = default.as_str().unwrap_or("STANDING");
            let pose_ident = Ident::new(&pose_str.to_upper_camel_case(), Span::call_site());
            quote! { EntityPose::#pose_ident }
        }
        "direction" => {
            let dir_str = default.as_str().unwrap_or("DOWN");
            let dir_ident = Ident::new(&dir_str.to_upper_camel_case(), Span::call_site());
            quote! { Direction::#dir_ident }
        }
        "rotations" => {
            if let Some(obj) = default.as_object() {
                let x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let z = obj.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let x_lit = Literal::f32_suffixed(x);
                let y_lit = Literal::f32_suffixed(y);
                let z_lit = Literal::f32_suffixed(z);
                quote! { Rotations::new(#x_lit, #y_lit, #z_lit) }
            } else {
                quote! { Rotations::ZERO }
            }
        }
        "block_pos" => {
            if let Some(obj) = default.as_object() {
                let x = obj.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let y = obj.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let z = obj.get("z").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                quote! { BlockPos::new(#x, #y, #z) }
            } else {
                quote! { BlockPos::new(0, 0, 0) }
            }
        }
        "block_state" => {
            let v = default.as_i64().unwrap_or(0) as u16;
            quote! { BlockStateId(#v) }
        }
        "component" => {
            quote! { Box::new(TextComponent::default()) }
        }
        // Variant types use VarInt registry IDs
        "cat_variant"
        | "cow_variant"
        | "wolf_variant"
        | "wolf_sound_variant"
        | "frog_variant"
        | "pig_variant"
        | "chicken_variant"
        | "zombie_nautilus_variant"
        | "painting_variant"
        | "copper_golem_state"
        | "weathering_copper_state" => {
            let v = default.as_i64().unwrap_or(0) as i32;
            quote! { #v }
        }
        "sniffer_state" => {
            quote! { SnifferState::default() }
        }
        "armadillo_state" => {
            quote! { ArmadilloState::default() }
        }
        "vector3" => {
            if let Some(obj) = default.as_object() {
                let x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let z = obj.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let x_lit = Literal::f32_suffixed(x);
                let y_lit = Literal::f32_suffixed(y);
                let z_lit = Literal::f32_suffixed(z);
                quote! { Vector3f::new(#x_lit, #y_lit, #z_lit) }
            } else {
                quote! { Vector3f::ZERO }
            }
        }
        "quaternion" => {
            if let Some(obj) = default.as_object() {
                let x = obj.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let y = obj.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let z = obj.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let w = obj.get("w").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
                let x_lit = Literal::f32_suffixed(x);
                let y_lit = Literal::f32_suffixed(y);
                let z_lit = Literal::f32_suffixed(z);
                let w_lit = Literal::f32_suffixed(w);
                quote! { Quaternionf::new(#x_lit, #y_lit, #z_lit, #w_lit) }
            } else {
                quote! { Quaternionf::IDENTITY }
            }
        }
        "villager_data" => {
            if let Some(obj) = default.as_object() {
                let vt = obj.get("type").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let prof = obj.get("profession").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                let level = obj.get("level").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
                quote! { VillagerData::new(#vt, #prof, #level) }
            } else {
                quote! { VillagerData::new(0, 0, 1) }
            }
        }
        "humanoid_arm" => {
            quote! { HumanoidArm::default() }
        }
        "item_stack" => {
            quote! { ItemStack::empty() }
        }
        "particle" => {
            quote! { ParticleData::default() }
        }
        "particles" => {
            quote! { ParticleList::default() }
        }
        "resolvable_profile" => {
            quote! { ResolvableProfile::default() }
        }
        _ => quote! { Default::default() },
    }
}

/// Generate the EntityData conversion expression for packing.
fn entity_data_expr(serializer: &str, field_ident: &Ident) -> TokenStream {
    let (_, variant, _) = serializer_info(serializer).unwrap();
    let variant_ident = Ident::new(variant, Span::call_site());

    match serializer {
        // Copy types
        "byte"
        | "int"
        | "long"
        | "float"
        | "boolean"
        | "cat_variant"
        | "cow_variant"
        | "wolf_variant"
        | "wolf_sound_variant"
        | "frog_variant"
        | "pig_variant"
        | "chicken_variant"
        | "zombie_nautilus_variant"
        | "painting_variant"
        | "copper_golem_state"
        | "weathering_copper_state" => {
            quote! { EntityData::#variant_ident(*self.#field_ident.get()) }
        }
        // BlockStateId and Direction are Copy
        "block_state" | "direction" | "pose" | "sniffer_state" | "armadillo_state"
        | "humanoid_arm" => {
            quote! { EntityData::#variant_ident(*self.#field_ident.get()) }
        }
        // Clone types
        "string"
        | "component"
        | "optional_component"
        | "optional_block_pos"
        | "optional_block_state"
        | "optional_living_entity_reference"
        | "optional_unsigned_int"
        | "optional_global_pos"
        | "item_stack"
        | "particle"
        | "particles"
        | "resolvable_profile"
        | "villager_data" => {
            quote! { EntityData::#variant_ident(self.#field_ident.get().clone()) }
        }
        // Copy structs
        "rotations" | "block_pos" | "vector3" | "quaternion" => {
            quote! { EntityData::#variant_ident(*self.#field_ident.get()) }
        }
        _ => quote! { EntityData::Byte(0) }, // Fallback
    }
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/entities.json");

    let entities_file = "build_assets/entities.json";
    let content = fs::read_to_string(entities_file).unwrap();
    let entities: Vec<EntityEntry> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse entities.json: {e}"));

    let mut stream = TokenStream::new();

    // Imports
    stream.extend(quote! {
        use crate::entity_data::{
            ArmadilloState, BlockPos, DataValue, Direction, EntityData, EntityPose,
            GlobalPos, HumanoidArm, ParticleData, ParticleList, Quaternionf,
            ResolvableProfile, Rotations, SnifferState, SyncedValue, Vector3f,
            VillagerData,
        };
        use crate::item_stack::ItemStack;
        use steel_utils::BlockStateId;
        use text_components::TextComponent;
        use uuid::Uuid;
    });

    // Generate a struct for each entity
    for entity in &entities {
        // Skip entities with no synched_data
        if entity.synched_data.is_empty() {
            continue;
        }

        let struct_name = format!("{}EntityData", entity.name.to_upper_camel_case());
        let struct_ident = Ident::new(&struct_name, Span::call_site());

        // Generate fields
        let mut field_defs = Vec::new();
        let mut field_inits = Vec::new();
        let mut pack_dirty_checks = Vec::new();
        let mut pack_all_entries = Vec::new();
        let mut is_dirty_checks = Vec::new();

        for data in &entity.synched_data {
            let Some((rust_type, _, serializer_id)) = serializer_info(&data.serializer) else {
                eprintln!(
                    "Warning: Unknown serializer '{}' for entity '{}' field '{}'",
                    data.serializer, entity.name, data.name
                );
                continue;
            };

            // Clean up field name (remove _id suffix, convert to snake_case)
            let field_name = data.name.trim_end_matches("_id").to_snake_case();
            // Handle Rust reserved keywords
            let field_name = match field_name.as_str() {
                "type" => "variant_type".to_string(),
                "self" => "self_ref".to_string(),
                "super" => "super_ref".to_string(),
                "crate" => "crate_ref".to_string(),
                "mod" => "mod_ref".to_string(),
                "ref" => "ref_value".to_string(),
                "move" => "move_value".to_string(),
                other => other.to_string(),
            };
            let field_ident = Ident::new(&field_name, Span::call_site());
            let rust_type_tokens: TokenStream = rust_type.parse().unwrap();
            let default_expr = default_value_expr(&data.serializer, &data.default_value);
            let index = data.index;
            let serializer_id_lit = serializer_id;
            let entity_data_expr = entity_data_expr(&data.serializer, &field_ident);

            field_defs.push(quote! {
                pub #field_ident: SyncedValue<#rust_type_tokens>
            });

            field_inits.push(quote! {
                #field_ident: SyncedValue::new(#default_expr)
            });

            pack_dirty_checks.push(quote! {
                if self.#field_ident.is_dirty() {
                    values.push(DataValue {
                        index: #index,
                        serializer_id: #serializer_id_lit,
                        value: #entity_data_expr,
                    });
                    self.#field_ident.clear_dirty();
                }
            });

            pack_all_entries.push(quote! {
                if !self.#field_ident.is_default() {
                    values.push(DataValue {
                        index: #index,
                        serializer_id: #serializer_id_lit,
                        value: #entity_data_expr,
                    });
                }
            });

            is_dirty_checks.push(quote! {
                self.#field_ident.is_dirty()
            });
        }

        // Generate the struct
        stream.extend(quote! {
            /// Entity data for `#struct_name`.
            #[derive(Debug, Clone)]
            pub struct #struct_ident {
                #(#field_defs),*
            }

            impl #struct_ident {
                /// Create new entity data with default values.
                pub fn new() -> Self {
                    Self {
                        #(#field_inits),*
                    }
                }

                /// Pack all dirty values for network sync, clearing dirty flags.
                /// Returns `None` if no values are dirty.
                pub fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
                    let mut values = Vec::new();
                    #(#pack_dirty_checks)*
                    if values.is_empty() { None } else { Some(values) }
                }

                /// Pack all non-default values (for initial entity spawn).
                pub fn pack_all(&self) -> Vec<DataValue> {
                    let mut values = Vec::new();
                    #(#pack_all_entries)*
                    values
                }

                /// Returns `true` if any field has been modified.
                pub fn is_dirty(&self) -> bool {
                    #(#is_dirty_checks)||*
                }
            }

            impl Default for #struct_ident {
                fn default() -> Self {
                    Self::new()
                }
            }
        });
    }

    stream
}
