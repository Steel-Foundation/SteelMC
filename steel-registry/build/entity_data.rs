//! Build script for generating entity data structs from entities.json.
//!
//! Generates composed data structs matching the vanilla class layers that declare
//! synchronized entity data.

use std::fs;

use crate::generator_functions::vanilla_variant_id;
use heck::{ToShoutySnakeCase, ToSnakeCase, ToUpperCamelCase};
use proc_macro2::{Ident, Literal, Span, TokenStream};
use quote::quote;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
struct EntityEntry {
    #[expect(
        dead_code,
        reason = "extracted entity data keeps vanilla ids for validation context"
    )]
    id: i32,
    name: String,
    synched_data: SynchedData,
}

#[derive(Deserialize, Debug)]
struct SynchedData {
    #[expect(
        dead_code,
        reason = "class metadata is kept from the extractor for future validation"
    )]
    java_class: String,
    #[expect(
        dead_code,
        reason = "class metadata is kept from the extractor for future validation"
    )]
    class_hierarchy: Vec<ClassEntry>,
    layers: Vec<SynchedDataLayer>,
}

#[derive(Deserialize, Debug)]
struct ClassEntry {
    #[expect(
        dead_code,
        reason = "class metadata is kept from the extractor for future validation"
    )]
    java_class: String,
    #[expect(
        dead_code,
        reason = "class metadata is kept from the extractor for future validation"
    )]
    simple_name: String,
}

#[derive(Deserialize, Debug)]
struct SynchedDataLayer {
    java_class: String,
    simple_name: String,
    fields: Vec<SynchedDataEntry>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
struct SynchedDataEntry {
    index: u8,
    name: String,
    accessor_field: String,
    serializer_id: i32,
    serializer: String,
    #[serde(default)]
    default_value: Value,
}

#[derive(Clone, Debug)]
struct LayerDefinition {
    java_class: String,
    simple_name: String,
    parent_java_class: Option<String>,
    fields: Vec<SynchedDataEntry>,
}

/// Maps a serializer name to (Rust type, `EntityData` variant, vanilla serializer ID).
fn serializer_info(serializer: &str) -> Option<(&'static str, &'static str, i32)> {
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
        "cat_sound_variant" => ("i32", "CatSoundVariant", 22),
        "cow_variant" => ("i32", "CowVariant", 23),
        "cow_sound_variant" => ("i32", "CowSoundVariant", 24),
        "wolf_variant" => ("i32", "WolfVariant", 25),
        "wolf_sound_variant" => ("i32", "WolfSoundVariant", 26),
        "frog_variant" => ("i32", "FrogVariant", 27),
        "pig_variant" => ("i32", "PigVariant", 28),
        "pig_sound_variant" => ("i32", "PigSoundVariant", 29),
        "chicken_variant" => ("i32", "ChickenVariant", 30),
        "chicken_sound_variant" => ("i32", "ChickenSoundVariant", 31),
        "zombie_nautilus_variant" => ("i32", "ZombieNautilusVariant", 32),
        "optional_global_pos" => ("Option<GlobalPos>", "OptionalGlobalPos", 33),
        "painting_variant" => ("i32", "PaintingVariant", 34),
        "sniffer_state" => ("SnifferState", "SnifferState", 35),
        "armadillo_state" => ("ArmadilloState", "ArmadilloState", 36),
        "copper_golem_state" => ("i32", "CopperGolemState", 37),
        "weathering_copper_state" => ("i32", "WeatheringCopperState", 38),
        "vector3" => ("Vector3f", "Vector3", 39),
        "quaternion" => ("Quaternionf", "Quaternion", 40),
        "resolvable_profile" => ("ResolvableProfile", "ResolvableProfile", 41),
        "humanoid_arm" => ("HumanoidArm", "HumanoidArm", 42),
        _ => return None,
    })
}

fn required_string<'a>(default: &'a Value, serializer: &str) -> &'a str {
    default
        .as_str()
        .unwrap_or_else(|| panic!("Expected string default for {serializer}, got {default}"))
}

fn minecraft_path<'a>(key: &'a str, serializer: &str) -> &'a str {
    key.strip_prefix("minecraft:")
        .unwrap_or_else(|| panic!("Expected minecraft namespaced key for {serializer}, got {key}"))
}

fn key_ident(default: &Value, serializer: &str) -> Ident {
    let path = minecraft_path(required_string(default, serializer), serializer);
    Ident::new(&path.to_shouty_snake_case(), Span::call_site())
}

fn key_field_ident(default: &Value, serializer: &str) -> Ident {
    let path = minecraft_path(required_string(default, serializer), serializer);
    Ident::new(&path.to_snake_case(), Span::call_site())
}

fn required_i64(default: &Value, serializer: &str) -> i64 {
    default
        .as_i64()
        .unwrap_or_else(|| panic!("Expected integer default for {serializer}, got {default}"))
}

fn required_i32(default: &Value, serializer: &str) -> i32 {
    required_i64(default, serializer)
        .try_into()
        .unwrap_or_else(|_| {
            panic!("Integer default for {serializer} is out of i32 range: {default}")
        })
}

fn required_i8(default: &Value, serializer: &str) -> i8 {
    required_i64(default, serializer)
        .try_into()
        .unwrap_or_else(|_| {
            panic!("Integer default for {serializer} is out of i8 range: {default}")
        })
}

fn required_f32(default: &Value, serializer: &str) -> f32 {
    default
        .as_f64()
        .unwrap_or_else(|| panic!("Expected float default for {serializer}, got {default}"))
        as f32
}

fn required_object<'a>(default: &'a Value, serializer: &str) -> &'a serde_json::Map<String, Value> {
    default
        .as_object()
        .unwrap_or_else(|| panic!("Expected object default for {serializer}, got {default}"))
}

fn required_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    serializer: &str,
    field: &str,
) -> &'a Value {
    object
        .get(field)
        .unwrap_or_else(|| panic!("Missing '{field}' in {serializer} default: {object:?}"))
}

fn required_object_i32(
    object: &serde_json::Map<String, Value>,
    serializer: &str,
    field: &str,
) -> i32 {
    required_i32(required_field(object, serializer, field), serializer)
}

fn required_object_f32(
    object: &serde_json::Map<String, Value>,
    serializer: &str,
    field: &str,
) -> f32 {
    required_f32(required_field(object, serializer, field), serializer)
}

fn require_exact_string(default: &Value, serializer: &str, expected: &str) {
    let actual = required_string(default, serializer);
    assert_eq!(
        actual, expected,
        "Expected {serializer} default '{expected}', got '{actual}'"
    );
}

fn require_empty_array(default: &Value, serializer: &str) {
    let Some(values) = default.as_array() else {
        panic!("Expected empty array default for {serializer}, got {default}");
    };
    assert!(
        values.is_empty(),
        "Expected empty array default for {serializer}, got {default}"
    );
}

fn optional_none_expr(serializer: &str, default: &Value) -> TokenStream {
    let present = default
        .get("present")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("Expected {serializer} presence, got {default}"));
    assert!(
        !present,
        "Unsupported present default for {serializer}: {default}"
    );
    quote! { None }
}

fn variant_registry_default_expr(module: &str, default: &Value, serializer: &str) -> TokenStream {
    let default = required_string(default, serializer);
    let subdir = module
        .strip_prefix("vanilla_")
        .and_then(|name| name.strip_suffix('s'))
        .unwrap_or_else(|| panic!("Unsupported registry default module: {module}"));
    let id = Literal::i32_unsuffixed(vanilla_variant_id(subdir, default) as i32);
    quote! { #id }
}

#[derive(Deserialize)]
struct ExtractedRegistryEntry {
    id: i32,
    key: String,
}

fn extracted_registry_default_expr(asset: &str, default: &Value, serializer: &str) -> TokenStream {
    let key = required_string(default, serializer);
    let entries: Vec<ExtractedRegistryEntry> = crate::generator_functions::read_json_asset(asset);
    let id = entries
        .iter()
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("Unknown {serializer} registry default {key} in {asset}"))
        .id;
    let id = Literal::i32_unsuffixed(id);
    quote! { #id }
}

fn extracted_registry_field_default_expr(
    asset: &str,
    object: &serde_json::Map<String, Value>,
    serializer: &str,
    field: &str,
) -> TokenStream {
    extracted_registry_default_expr(asset, required_field(object, serializer, field), serializer)
}

fn ordinal_default_expr(default: &Value, serializer: &str, names: &[&str]) -> TokenStream {
    let name = required_string(default, serializer);
    let ordinal = names
        .iter()
        .position(|candidate| *candidate == name)
        .unwrap_or_else(|| panic!("Unknown {serializer} default: {name}")) as i32;
    quote! { #ordinal }
}

fn item_stack_default_expr(default: &Value) -> TokenStream {
    if default.as_str().is_some() {
        require_exact_string(default, "item_stack", "empty");
        return quote! { ItemStack::empty() };
    }

    let object = required_object(default, "item_stack");
    assert!(
        object.len() == 2,
        "Expected item_stack default with only item/count, got {default}"
    );

    let item_ident = key_field_ident(required_field(object, "item_stack", "item"), "item_stack");
    let count = required_object_i32(object, "item_stack", "count");
    assert!(
        count > 0,
        "Expected positive item_stack count, got {count} in {default}"
    );

    quote! {
        ItemStack::with_count(&crate::vanilla_items::ITEMS.#item_ident, #count)
    }
}

/// Generate the default value expression for a field.
fn default_value_expr(serializer: &str, default: &Value) -> TokenStream {
    match serializer {
        "byte" => {
            let v = required_i8(default, serializer);
            quote! { #v }
        }
        "int" => {
            let v = required_i32(default, serializer);
            quote! { #v }
        }
        "long" => {
            let v = required_i64(default, serializer);
            quote! { #v }
        }
        "float" => {
            let v = required_f32(default, serializer);
            let lit = Literal::f32_suffixed(v);
            quote! { #lit }
        }
        "string" => {
            let v = required_string(default, serializer);
            quote! { #v.to_string() }
        }
        "boolean" => {
            let v = default
                .as_bool()
                .unwrap_or_else(|| panic!("Expected boolean default, got {default}"));
            quote! { #v }
        }
        "optional_component" => {
            let present = default
                .get("present")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| panic!("Expected optional_component presence, got {default}"));
            if present {
                let text = default
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("Expected optional_component value, got {default}"));
                quote! { Some(Box::new(TextComponent::plain(#text))) }
            } else {
                quote! { None }
            }
        }
        "optional_block_pos"
        | "optional_block_state"
        | "optional_living_entity_reference"
        | "optional_unsigned_int"
        | "optional_global_pos" => optional_none_expr(serializer, default),
        "pose" => {
            let pose_str = required_string(default, serializer);
            let pose_ident = Ident::new(&pose_str.to_upper_camel_case(), Span::call_site());
            quote! { EntityPose::#pose_ident }
        }
        "direction" => {
            let dir_str = required_string(default, serializer);
            let dir_ident = Ident::new(&dir_str.to_upper_camel_case(), Span::call_site());
            quote! { Direction::#dir_ident }
        }
        "rotations" => {
            let obj = required_object(default, serializer);
            let x = required_object_f32(obj, serializer, "x");
            let y = required_object_f32(obj, serializer, "y");
            let z = required_object_f32(obj, serializer, "z");
            let x_lit = Literal::f32_suffixed(x);
            let y_lit = Literal::f32_suffixed(y);
            let z_lit = Literal::f32_suffixed(z);
            quote! { Rotations::new(#x_lit, #y_lit, #z_lit) }
        }
        "block_pos" => {
            let obj = required_object(default, serializer);
            let x = required_object_i32(obj, serializer, "x");
            let y = required_object_i32(obj, serializer, "y");
            let z = required_object_i32(obj, serializer, "z");
            quote! { BlockPos::new(#x, #y, #z) }
        }
        "block_state" => {
            if default.as_i64().is_some() {
                panic!(
                    "Raw numeric BlockStateId defaults are not allowed in generated entity data; use a block identifier for {serializer}"
                );
            } else {
                let block_ident = key_ident(default, serializer);
                quote! { crate::vanilla_blocks::#block_ident.default_state() }
            }
        }
        "component" => {
            let text = required_string(default, serializer);
            if text.is_empty() {
                quote! { Box::new(TextComponent::default()) }
            } else {
                quote! { Box::new(TextComponent::plain(#text)) }
            }
        }
        "cat_variant" => variant_registry_default_expr("vanilla_cat_variants", default, serializer),
        "cat_sound_variant" => {
            variant_registry_default_expr("vanilla_cat_sound_variants", default, serializer)
        }
        "cow_variant" => variant_registry_default_expr("vanilla_cow_variants", default, serializer),
        "cow_sound_variant" => {
            variant_registry_default_expr("vanilla_cow_sound_variants", default, serializer)
        }
        "wolf_variant" => {
            variant_registry_default_expr("vanilla_wolf_variants", default, serializer)
        }
        "wolf_sound_variant" => {
            variant_registry_default_expr("vanilla_wolf_sound_variants", default, serializer)
        }
        "frog_variant" => {
            variant_registry_default_expr("vanilla_frog_variants", default, serializer)
        }
        "pig_variant" => variant_registry_default_expr("vanilla_pig_variants", default, serializer),
        "pig_sound_variant" => {
            variant_registry_default_expr("vanilla_pig_sound_variants", default, serializer)
        }
        "chicken_variant" => {
            variant_registry_default_expr("vanilla_chicken_variants", default, serializer)
        }
        "chicken_sound_variant" => {
            variant_registry_default_expr("vanilla_chicken_sound_variants", default, serializer)
        }
        "zombie_nautilus_variant" => {
            variant_registry_default_expr("vanilla_zombie_nautilus_variants", default, serializer)
        }
        "painting_variant" => {
            variant_registry_default_expr("vanilla_painting_variants", default, serializer)
        }
        "copper_golem_state" => ordinal_default_expr(
            default,
            serializer,
            &[
                "IDLE",
                "GETTING_ITEM",
                "GETTING_NO_ITEM",
                "DROPPING_ITEM",
                "DROPPING_NO_ITEM",
            ],
        ),
        "weathering_copper_state" => ordinal_default_expr(
            default,
            serializer,
            &["UNAFFECTED", "EXPOSED", "WEATHERED", "OXIDIZED"],
        ),
        "humanoid_arm" => {
            let arm_str = required_string(default, serializer);
            let arm_ident = Ident::new(&arm_str.to_upper_camel_case(), Span::call_site());
            quote! { HumanoidArm::#arm_ident }
        }
        "sniffer_state" => {
            let state_str = required_string(default, serializer);
            let state_ident = Ident::new(&state_str.to_upper_camel_case(), Span::call_site());
            quote! { SnifferState::#state_ident }
        }
        "armadillo_state" => {
            let state_str = required_string(default, serializer);
            let state_ident = Ident::new(&state_str.to_upper_camel_case(), Span::call_site());
            quote! { ArmadilloState::#state_ident }
        }
        "vector3" => {
            let obj = required_object(default, serializer);
            let x = required_object_f32(obj, serializer, "x");
            let y = required_object_f32(obj, serializer, "y");
            let z = required_object_f32(obj, serializer, "z");
            let x_lit = Literal::f32_suffixed(x);
            let y_lit = Literal::f32_suffixed(y);
            let z_lit = Literal::f32_suffixed(z);
            quote! { Vector3f::new(#x_lit, #y_lit, #z_lit) }
        }
        "quaternion" => {
            let obj = required_object(default, serializer);
            let x = required_object_f32(obj, serializer, "x");
            let y = required_object_f32(obj, serializer, "y");
            let z = required_object_f32(obj, serializer, "z");
            let w = required_object_f32(obj, serializer, "w");
            let x_lit = Literal::f32_suffixed(x);
            let y_lit = Literal::f32_suffixed(y);
            let z_lit = Literal::f32_suffixed(z);
            let w_lit = Literal::f32_suffixed(w);
            quote! { Quaternionf::new(#x_lit, #y_lit, #z_lit, #w_lit) }
        }
        "villager_data" => {
            let obj = required_object(default, serializer);
            assert_eq!(
                obj.len(),
                3,
                "Expected villager_data default with type/profession/level, got {default}"
            );

            let vt = extracted_registry_field_default_expr(
                "build_assets/villager_types.json",
                obj,
                serializer,
                "type",
            );
            let prof = extracted_registry_field_default_expr(
                "build_assets/villager_professions.json",
                obj,
                serializer,
                "profession",
            );
            let level = required_object_i32(obj, serializer, "level");
            quote! { VillagerData::new(#vt, #prof, #level) }
        }
        "item_stack" => item_stack_default_expr(default),
        "particle" => {
            let obj = required_object(default, serializer);
            assert_eq!(
                obj.len(),
                2,
                "Expected particle default with type/options, got {default}"
            );

            let particle_type = extracted_registry_field_default_expr(
                "build_assets/particle_types.json",
                obj,
                serializer,
                "type",
            );
            let options = required_object(required_field(obj, serializer, "options"), serializer);
            let kind = required_string(required_field(options, serializer, "kind"), serializer);

            match kind {
                "none" => {
                    assert_eq!(
                        options.len(),
                        1,
                        "Expected particle none options to contain only kind, got {default}"
                    );
                    quote! { ParticleData::new(#particle_type, ParticleOptions::None) }
                }
                "color" => {
                    assert_eq!(
                        options.len(),
                        2,
                        "Expected particle color options with kind/color, got {default}"
                    );
                    let color = required_object_i32(options, serializer, "color");
                    quote! {
                        ParticleData::new(#particle_type, ParticleOptions::Color { color: #color })
                    }
                }
                _ => panic!("Unsupported particle options kind '{kind}' in {default}"),
            }
        }
        "particles" => {
            require_empty_array(default, serializer);
            quote! { ParticleList::default() }
        }
        "resolvable_profile" => {
            require_exact_string(default, serializer, "Static");
            quote! { ResolvableProfile::default() }
        }
        _ => panic!("Unhandled entity data default for serializer {serializer}: {default}"),
    }
}

/// Generate the `EntityData` conversion expression for packing.
fn entity_data_expr(serializer: &str, field_ident: &Ident) -> TokenStream {
    let (_, variant, _) = serializer_info(serializer)
        .unwrap_or_else(|| panic!("Unknown entity data serializer: {serializer}"));
    let variant_ident = Ident::new(variant, Span::call_site());

    match serializer {
        // Copy types
        "byte"
        | "int"
        | "long"
        | "float"
        | "boolean"
        | "cat_variant"
        | "cat_sound_variant"
        | "cow_variant"
        | "cow_sound_variant"
        | "wolf_variant"
        | "wolf_sound_variant"
        | "frog_variant"
        | "pig_variant"
        | "pig_sound_variant"
        | "chicken_variant"
        | "chicken_sound_variant"
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
        _ => panic!("Unhandled entity data serializer: {serializer}"),
    }
}

fn data_struct_name(simple_name: &str) -> String {
    if simple_name == "Entity" {
        "BaseEntityData".to_owned()
    } else if simple_name.ends_with("Entity") {
        format!("{simple_name}Data")
    } else {
        format!("{simple_name}EntityData")
    }
}

fn data_struct_ident(simple_name: &str) -> Ident {
    Ident::new(&data_struct_name(simple_name), Span::call_site())
}

fn sanitize_field_name(name: &str) -> String {
    let field_name = name.trim_end_matches("_id").to_snake_case();
    match field_name.as_str() {
        "type" => "variant_type".to_string(),
        "self" => "self_ref".to_string(),
        "super" => "super_ref".to_string(),
        "crate" => "crate_ref".to_string(),
        "mod" => "mod_ref".to_string(),
        "ref" => "ref_value".to_string(),
        "move" => "move_value".to_string(),
        other => other.to_string(),
    }
}

fn entity_struct_name(entity_name: &str) -> String {
    format!("{}EntityData", entity_name.to_upper_camel_case())
}

fn field_shape_matches(left: &SynchedDataEntry, right: &SynchedDataEntry) -> bool {
    left.index == right.index
        && left.name == right.name
        && left.accessor_field == right.accessor_field
        && left.serializer_id == right.serializer_id
        && left.serializer == right.serializer
}

fn parent_field_ident(simple_name: &str) -> Ident {
    let field_name = if simple_name == "Entity" {
        "base".to_owned()
    } else {
        sanitize_field_name(simple_name)
    };
    Ident::new(&field_name, Span::call_site())
}

fn layer_accessor_methods(
    root_layer: &LayerDefinition,
    layer_indices: &FxHashMap<&str, usize>,
    layers: &[LayerDefinition],
    root_expr: TokenStream,
    root_is_self: bool,
) -> Vec<TokenStream> {
    let mut methods = Vec::new();
    let mut current_layer = root_layer;
    let mut path = root_expr;
    let mut is_self_path = root_is_self;

    loop {
        let accessor_ident = parent_field_ident(&current_layer.simple_name);
        let accessor_mut_ident = Ident::new(&format!("{accessor_ident}_mut"), Span::call_site());
        let struct_ident = data_struct_ident(&current_layer.simple_name);
        let doc = format!(
            "Returns the `{}` layer.",
            data_struct_name(&current_layer.simple_name)
        );
        let doc_mut = format!(
            "Returns the mutable `{}` layer.",
            data_struct_name(&current_layer.simple_name)
        );
        let ref_path = path.clone();
        let mut_path = path.clone();
        let ref_body = if is_self_path {
            quote! { self }
        } else {
            quote! { &#ref_path }
        };
        let mut_body = if is_self_path {
            quote! { self }
        } else {
            quote! { &mut #mut_path }
        };

        methods.push(quote! {
            #[doc = #doc]
            pub fn #accessor_ident(&self) -> &#struct_ident {
                #ref_body
            }

            #[doc = #doc_mut]
            pub fn #accessor_mut_ident(&mut self) -> &mut #struct_ident {
                #mut_body
            }
        });

        let Some(parent_java_class) = current_layer.parent_java_class.as_ref() else {
            break;
        };
        let parent_index = layer_indices
            .get(parent_java_class.as_str())
            .unwrap_or_else(|| panic!("Missing parent entity data layer: {parent_java_class}"));
        let parent_layer = &layers[*parent_index];
        let parent_field_ident = parent_field_ident(&parent_layer.simple_name);
        path = quote! { #path.#parent_field_ident };
        is_self_path = false;
        current_layer = parent_layer;
    }

    methods
}

fn collect_layers(entities: &[EntityEntry]) -> Vec<LayerDefinition> {
    let mut layer_indices = FxHashMap::default();
    let mut layers = Vec::new();

    for entity in entities {
        let mut parent_java_class = None;
        for layer in &entity.synched_data.layers {
            if layer.fields.is_empty() {
                continue;
            }

            let mut fields = layer.fields.clone();
            fields.sort_by_key(|field| field.index);

            if let Some(&index) = layer_indices.get(&layer.java_class) {
                let existing: &LayerDefinition = &layers[index];
                if existing.simple_name != layer.simple_name
                    || existing.parent_java_class != parent_java_class
                    || existing.fields.len() != fields.len()
                    || !existing
                        .fields
                        .iter()
                        .zip(fields.iter())
                        .all(|(left, right)| field_shape_matches(left, right))
                {
                    panic!(
                        "Inconsistent entity data layer for {} while processing entity {}",
                        layer.java_class, entity.name
                    );
                }
            } else {
                layer_indices.insert(layer.java_class.clone(), layers.len());
                layers.push(LayerDefinition {
                    java_class: layer.java_class.clone(),
                    simple_name: layer.simple_name.clone(),
                    parent_java_class: parent_java_class.clone(),
                    fields,
                });
            }

            parent_java_class = Some(layer.java_class.clone());
        }
    }

    layers
}

fn field_path_for_layer(layers: &[&SynchedDataLayer], target_index: usize) -> TokenStream {
    let mut path = quote! { data };

    for current_index in (target_index + 1..layers.len()).rev() {
        let field_ident = parent_field_ident(&layers[current_index - 1].simple_name);
        path = quote! { #path.#field_ident };
    }

    path
}

fn concrete_default_overrides(
    entity: &EntityEntry,
    canonical_layers: &FxHashMap<&str, usize>,
    layers: &[LayerDefinition],
) -> Vec<TokenStream> {
    let entity_layers: Vec<_> = entity
        .synched_data
        .layers
        .iter()
        .filter(|layer| !layer.fields.is_empty())
        .collect();
    let mut overrides = Vec::new();

    for (layer_index, entity_layer) in entity_layers.iter().enumerate() {
        let canonical_index = canonical_layers
            .get(entity_layer.java_class.as_str())
            .unwrap_or_else(|| panic!("Missing canonical layer {}", entity_layer.java_class));
        let canonical_layer = &layers[*canonical_index];
        let layer_path = field_path_for_layer(&entity_layers, layer_index);

        for field in &entity_layer.fields {
            let Some(canonical_field) = canonical_layer
                .fields
                .iter()
                .find(|candidate| candidate.index == field.index)
            else {
                panic!(
                    "Missing canonical field {} on layer {}",
                    field.name, entity_layer.java_class
                );
            };

            if canonical_field.default_value == field.default_value {
                continue;
            }

            let field_ident = Ident::new(&sanitize_field_name(&field.name), Span::call_site());
            let default_expr = default_value_expr(&field.serializer, &field.default_value);
            overrides.push(quote! {
                #layer_path.#field_ident = SyncedValue::new(#default_expr);
            });
        }
    }

    overrides
}

fn layer_has_living_entity_data(
    layer: &LayerDefinition,
    layer_indices: &FxHashMap<&str, usize>,
    layers: &[LayerDefinition],
) -> bool {
    let mut current_layer = layer;
    loop {
        if current_layer.simple_name == "LivingEntity" {
            return true;
        }

        let Some(parent_java_class) = current_layer.parent_java_class.as_ref() else {
            return false;
        };
        let Some(parent_index) = layer_indices.get(parent_java_class.as_str()) else {
            return false;
        };
        current_layer = &layers[*parent_index];
    }
}

fn entity_has_living_entity_data(entity: &EntityEntry) -> bool {
    entity
        .synched_data
        .layers
        .iter()
        .any(|layer| !layer.fields.is_empty() && layer.simple_name == "LivingEntity")
}

pub(crate) fn build() -> TokenStream {
    println!("cargo:rerun-if-changed=build_assets/entities.json");

    let entities_file = "build_assets/entities.json";
    let content = fs::read_to_string(entities_file)
        .unwrap_or_else(|e| panic!("Failed to read {entities_file}: {e}"));
    let entities: Vec<EntityEntry> = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("Failed to parse entities.json: {e}"));
    let layers = collect_layers(&entities);
    let layer_indices: FxHashMap<_, _> = layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (layer.java_class.as_str(), index))
        .collect();
    let mut layer_new_overrides = FxHashMap::default();

    for entity in &entities {
        let Some(last_layer) = entity
            .synched_data
            .layers
            .iter()
            .rev()
            .find(|layer| !layer.fields.is_empty())
        else {
            continue;
        };

        if entity_struct_name(&entity.name) == data_struct_name(&last_layer.simple_name) {
            let overrides = concrete_default_overrides(entity, &layer_indices, &layers);
            if !overrides.is_empty() {
                layer_new_overrides.insert(last_layer.java_class.as_str(), overrides);
            }
        }
    }

    let mut stream = TokenStream::new();

    // Imports
    stream.extend(quote! {
        use crate::entity_data::{
            ArmadilloState, BlockPos, DataValue, Direction, EntityData, EntityPose,
            GlobalPos, HumanoidArm, ParticleData, ParticleList, ParticleOptions, Quaternionf,
            ResolvableProfile, Rotations, SnifferState, SyncedValue, Vector3f,
            VillagerData,
        };
        use crate::item_stack::ItemStack;
        use steel_utils::BlockStateId;
        use text_components::TextComponent;
        use uuid::Uuid;
        use crate::RegistryEntry;

        /// Common access to the vanilla synchronized entity data root layer.
        pub trait VanillaEntityData {
            /// Returns the shared vanilla base entity-data layer.
            fn base(&self) -> &BaseEntityData;

            /// Returns the mutable shared vanilla base entity-data layer.
            fn base_mut(&mut self) -> &mut BaseEntityData;

            /// Packs dirty values for network sync, clearing dirty flags.
            fn pack_dirty(&mut self) -> Option<Vec<DataValue>>;

            /// Packs all non-default values for initial entity spawn.
            fn pack_all(&self) -> Vec<DataValue>;

            /// Returns `true` if any field has been modified.
            fn is_dirty(&self) -> bool;
        }

        /// Common access to vanilla synchronized data declared by `LivingEntity`.
        pub trait VanillaLivingEntityData: VanillaEntityData {
            /// Returns the vanilla living entity-data layer.
            fn living_entity(&self) -> &LivingEntityData;

            /// Returns the mutable vanilla living entity-data layer.
            fn living_entity_mut(&mut self) -> &mut LivingEntityData;
        }
    });

    for layer in &layers {
        let struct_ident = data_struct_ident(&layer.simple_name);
        let new_overrides = layer_new_overrides
            .get(layer.java_class.as_str())
            .cloned()
            .unwrap_or_default();

        // Generate fields
        let mut field_defs = Vec::new();
        let mut field_inits = Vec::new();
        let mut pack_dirty_checks = Vec::new();
        let mut pack_all_entries = Vec::new();
        let mut is_dirty_checks = Vec::new();

        let parent_layer = layer.parent_java_class.as_ref().map(|parent_java_class| {
            let parent_index = layer_indices
                .get(parent_java_class.as_str())
                .unwrap_or_else(|| panic!("Missing parent entity data layer: {parent_java_class}"));
            &layers[*parent_index]
        });

        if let Some(parent_layer) = parent_layer {
            let parent_field_ident = parent_field_ident(&parent_layer.simple_name);
            let parent_struct_ident = data_struct_ident(&parent_layer.simple_name);

            field_defs.push(quote! {
                pub #parent_field_ident: #parent_struct_ident
            });
            field_inits.push(quote! {
                #parent_field_ident: #parent_struct_ident::new()
            });
            pack_dirty_checks.push(quote! {
                self.#parent_field_ident.pack_dirty_into(values);
            });
            pack_all_entries.push(quote! {
                self.#parent_field_ident.pack_all_into(values);
            });
            is_dirty_checks.push(quote! {
                self.#parent_field_ident.is_dirty()
            });
        }

        for data in &layer.fields {
            let (rust_type, _, expected_serializer_id) = serializer_info(&data.serializer)
                .unwrap_or_else(|| {
                    panic!(
                        "Unknown serializer '{}' for entity data layer '{}' field '{}'",
                        data.serializer, layer.simple_name, data.name
                    )
                });
            assert_eq!(
                data.serializer_id,
                expected_serializer_id,
                "Serializer '{}' for entity data layer '{}' field '{}' has id {}, expected {}",
                data.serializer,
                layer.simple_name,
                data.name,
                data.serializer_id,
                expected_serializer_id
            );

            let field_name = sanitize_field_name(&data.name);
            let field_ident = Ident::new(&field_name, Span::call_site());
            let rust_type_tokens: TokenStream = rust_type.parse().unwrap_or_else(|error| {
                panic!("Failed to parse Rust type '{rust_type}' for entity data: {error}")
            });
            let default_expr = default_value_expr(&data.serializer, &data.default_value);
            let index = data.index;
            let serializer_id_lit = data.serializer_id;
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

        let is_dirty_expr = if is_dirty_checks.is_empty() {
            quote! { false }
        } else {
            quote! { #(#is_dirty_checks)||* }
        };
        let new_body = if new_overrides.is_empty() {
            quote! {
                Self {
                    #(#field_inits),*
                }
            }
        } else {
            quote! {
                let mut data = Self {
                    #(#field_inits),*
                };
                #(#new_overrides)*
                data
            }
        };
        let layer_accessors =
            layer_accessor_methods(layer, &layer_indices, &layers, quote! { self }, true);
        let living_entity_data_impl =
            if layer_has_living_entity_data(layer, &layer_indices, &layers) {
                quote! {
                    impl VanillaLivingEntityData for #struct_ident {
                        fn living_entity(&self) -> &LivingEntityData {
                            #struct_ident::living_entity(self)
                        }

                        fn living_entity_mut(&mut self) -> &mut LivingEntityData {
                            #struct_ident::living_entity_mut(self)
                        }
                    }
                }
            } else {
                quote! {}
            };

        // Generate the struct
        stream.extend(quote! {
            /// Synchronized entity data declared by the vanilla `#struct_name` layer.
            #[derive(Debug, Clone)]
            pub struct #struct_ident {
                #(#field_defs),*
            }

            impl #struct_ident {
                /// Create new entity data with default values.
                pub fn new() -> Self {
                    #new_body
                }

                #(#layer_accessors)*

                /// Pack all dirty values for network sync, clearing dirty flags.
                /// Returns `None` if no values are dirty.
                pub fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
                    let mut values = Vec::new();
                    self.pack_dirty_into(&mut values);
                    if values.is_empty() { None } else { Some(values) }
                }

                fn pack_dirty_into(&mut self, values: &mut Vec<DataValue>) {
                    #(#pack_dirty_checks)*
                }

                /// Pack all non-default values (for initial entity spawn).
                pub fn pack_all(&self) -> Vec<DataValue> {
                    let mut values = Vec::new();
                    self.pack_all_into(&mut values);
                    values
                }

                fn pack_all_into(&self, values: &mut Vec<DataValue>) {
                    #(#pack_all_entries)*
                }

                /// Returns `true` if any field has been modified.
                pub fn is_dirty(&self) -> bool {
                    #is_dirty_expr
                }
            }

            impl Default for #struct_ident {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl VanillaEntityData for #struct_ident {
                fn base(&self) -> &BaseEntityData {
                    #struct_ident::base(self)
                }

                fn base_mut(&mut self) -> &mut BaseEntityData {
                    #struct_ident::base_mut(self)
                }

                fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
                    #struct_ident::pack_dirty(self)
                }

                fn pack_all(&self) -> Vec<DataValue> {
                    #struct_ident::pack_all(self)
                }

                fn is_dirty(&self) -> bool {
                    #struct_ident::is_dirty(self)
                }
            }

            #living_entity_data_impl
        });
    }

    for entity in &entities {
        let Some(last_layer) = entity
            .synched_data
            .layers
            .iter()
            .rev()
            .find(|layer| !layer.fields.is_empty())
        else {
            continue;
        };

        let concrete_struct_name = entity_struct_name(&entity.name);
        let layer_struct_name = data_struct_name(&last_layer.simple_name);
        if concrete_struct_name == layer_struct_name {
            continue;
        }

        let concrete_ident = Ident::new(&concrete_struct_name, Span::call_site());
        let layer_ident = data_struct_ident(&last_layer.simple_name);
        let root_field_ident = parent_field_ident(&last_layer.simple_name);
        let overrides = concrete_default_overrides(entity, &layer_indices, &layers);
        let layer_accessors = {
            let root_expr = quote! { self.#root_field_ident };
            layer_accessor_methods(
                &layers[*layer_indices
                    .get(last_layer.java_class.as_str())
                    .unwrap_or_else(|| panic!("Missing layer {}", last_layer.java_class))],
                &layer_indices,
                &layers,
                root_expr,
                false,
            )
        };
        let doc = format!(
            "Concrete synchronized entity data for vanilla entity `{}`.",
            entity.name
        );
        let new_body = if overrides.is_empty() {
            quote! {
                Self {
                    #root_field_ident: #layer_ident::new()
                }
            }
        } else {
            quote! {
                let mut data = #layer_ident::new();
                #(#overrides)*
                Self {
                    #root_field_ident: data
                }
            }
        };
        let living_entity_data_impl = if entity_has_living_entity_data(entity) {
            quote! {
                impl VanillaLivingEntityData for #concrete_ident {
                    fn living_entity(&self) -> &LivingEntityData {
                        #concrete_ident::living_entity(self)
                    }

                    fn living_entity_mut(&mut self) -> &mut LivingEntityData {
                        #concrete_ident::living_entity_mut(self)
                    }
                }
            }
        } else {
            quote! {}
        };

        stream.extend(quote! {
            #[doc = #doc]
            #[derive(Debug, Clone)]
            pub struct #concrete_ident {
                pub #root_field_ident: #layer_ident
            }

            impl #concrete_ident {
                /// Create new entity data with default values.
                pub fn new() -> Self {
                    #new_body
                }

                #(#layer_accessors)*

                /// Pack all dirty values for network sync, clearing dirty flags.
                /// Returns `None` if no values are dirty.
                pub fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
                    self.#root_field_ident.pack_dirty()
                }

                /// Pack all non-default values (for initial entity spawn).
                pub fn pack_all(&self) -> Vec<DataValue> {
                    self.#root_field_ident.pack_all()
                }

                /// Returns `true` if any field has been modified.
                pub fn is_dirty(&self) -> bool {
                    self.#root_field_ident.is_dirty()
                }
            }

            impl Default for #concrete_ident {
                fn default() -> Self {
                    Self::new()
                }
            }

            impl VanillaEntityData for #concrete_ident {
                fn base(&self) -> &BaseEntityData {
                    #concrete_ident::base(self)
                }

                fn base_mut(&mut self) -> &mut BaseEntityData {
                    #concrete_ident::base_mut(self)
                }

                fn pack_dirty(&mut self) -> Option<Vec<DataValue>> {
                    #concrete_ident::pack_dirty(self)
                }

                fn pack_all(&self) -> Vec<DataValue> {
                    #concrete_ident::pack_all(self)
                }

                fn is_dirty(&self) -> bool {
                    #concrete_ident::is_dirty(self)
                }
            }

            #living_entity_data_impl
        });
    }

    stream
}
