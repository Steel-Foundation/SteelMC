use crate::shared_structs::{
    BiomeCondition, BiomeConditionTarget, SpawnConditionEntry, TextComponentJson,
};
use heck::ToShoutySnakeCase;
use proc_macro2::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use std::fs;
use steel_utils::Identifier;

pub fn read_json_asset<T: serde::de::DeserializeOwned>(path: &str) -> T {
    println!("cargo:rerun-if-changed={path}");
    let content = fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    serde_json::from_str(&content).unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"))
}

pub fn sort_contiguous_registry_entries<T>(
    entries: &mut [T],
    path: &str,
    id: impl Fn(&T) -> usize,
) {
    entries.sort_by_key(&id);
    for (expected, entry) in entries.iter().enumerate() {
        let actual = id(entry);
        assert_eq!(
            actual, expected,
            "Expected contiguous ids in {path}: entry at position {expected} has id {actual}"
        );
    }
}

pub fn generate_identifier(resource: &Identifier) -> TokenStream {
    let namespace = resource.namespace.as_ref();
    let path = resource.path.as_ref();
    quote! { Identifier { namespace: Cow::Borrowed(#namespace), path: Cow::Borrowed(#path) } }
}

pub fn generate_sound_event_ref(resource: &Identifier) -> TokenStream {
    assert_eq!(
        resource.namespace.as_ref(),
        "minecraft",
        "vanilla sound event references must use the minecraft namespace: {resource}"
    );

    let ident = Ident::new(&resource.path.to_shouty_snake_case(), Span::call_site());
    quote! { &crate::sound_events::#ident }
}

pub fn generate_option<T, F>(opt: &Option<T>, f: F) -> TokenStream
where
    F: FnOnce(&T) -> TokenStream,
{
    match opt {
        Some(val) => {
            let inner = f(val);
            quote! { Some(#inner) }
        }
        None => quote! { None },
    }
}

pub fn generate_vec<T, F>(vec: &[T], f: F) -> TokenStream
where
    F: Fn(&T) -> TokenStream,
{
    let items: Vec<_> = vec.iter().map(f).collect();
    quote! { vec![#(#items),*] }
}

pub fn generate_biome_condition(condition: &BiomeCondition) -> TokenStream {
    let condition_type = condition.condition_type.as_str();
    let biomes = generate_biome_condition_target(&condition.biomes);

    quote! {
        BiomeCondition {
            condition_type: #condition_type,
            biomes: #biomes,
        }
    }
}

fn generate_biome_condition_target(target: &BiomeConditionTarget) -> TokenStream {
    match target {
        BiomeConditionTarget::Tag(tag) => {
            let tag = generate_identifier(tag);
            quote! { crate::shared_structs::BiomeConditionTarget::Tag(#tag) }
        }
        BiomeConditionTarget::Direct(biome) => {
            let biome = generate_identifier(biome);
            quote! { crate::shared_structs::BiomeConditionTarget::Direct(#biome) }
        }
    }
}

pub fn generate_spawn_condition_entry(entry: &SpawnConditionEntry) -> TokenStream {
    let priority = entry.priority;
    let condition = generate_option(&entry.condition, generate_biome_condition);

    quote! {
        SpawnConditionEntry {
            priority: #priority,
            condition: #condition,
        }
    }
}
pub fn generate_text_component(component: &TextComponentJson) -> TokenStream {
    let translate = component.translate.as_str();
    quote! {
        TextComponent::translated(TranslatedMessage::new(#translate, None))
    }
}

pub fn read_variants_from_dir<T: serde::de::DeserializeOwned>(subdir: &str) -> Vec<(String, T)> {
    let dir = format!("../steel-utils/build_assets/builtin_datapacks/minecraft/{subdir}");
    println!("cargo:rerun-if-changed={dir}/");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("Failed to read {dir}: {e}")) {
        let path = entry
            .unwrap_or_else(|e| panic!("Failed to read entry in {dir}: {e}"))
            .path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_else(|| panic!("Invalid variant file name in {dir}: {}", path.display()))
            .to_string();
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {}: {e}", path.display()));
        let value: T = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", name, e));
        out.push((name, value));
    }
    let order = vanilla_variant_order(subdir);
    out.sort_by_key(|(name, _)| {
        order
            .iter()
            .position(|ordered| *ordered == name)
            .unwrap_or_else(|| panic!("Unknown vanilla {subdir} variant in extracted data: {name}"))
    });
    assert_eq!(
        out.len(),
        order.len(),
        "Expected {} vanilla {subdir} variants, got {}",
        order.len(),
        out.len()
    );
    out
}

pub fn vanilla_variant_id(subdir: &str, key: &str) -> usize {
    let path = key.strip_prefix("minecraft:").unwrap_or(key);
    vanilla_variant_order(subdir)
        .iter()
        .position(|ordered| *ordered == path)
        .unwrap_or_else(|| panic!("Unknown vanilla {subdir} variant default: {key}"))
}

fn vanilla_variant_order(subdir: &str) -> &'static [&'static str] {
    match subdir {
        "cat_variant" => &[
            "tabby",
            "black",
            "red",
            "siamese",
            "british_shorthair",
            "calico",
            "persian",
            "ragdoll",
            "white",
            "jellie",
            "all_black",
        ],
        "cat_sound_variant" => &["classic", "royal"],
        "cow_variant" => &["temperate", "warm", "cold"],
        "cow_sound_variant" => &["classic", "moody"],
        "wolf_variant" => &[
            "pale", "spotted", "snowy", "black", "ashen", "rusty", "woods", "chestnut", "striped",
        ],
        "wolf_sound_variant" => &["classic", "puglin", "sad", "angry", "grumpy", "big", "cute"],
        "frog_variant" => &["temperate", "warm", "cold"],
        "pig_variant" => &["temperate", "warm", "cold"],
        "pig_sound_variant" => &["classic", "big", "mini"],
        "chicken_variant" => &["temperate", "warm", "cold"],
        "chicken_sound_variant" => &["classic", "picky"],
        "zombie_nautilus_variant" => &["temperate", "warm"],
        "painting_variant" => &[
            "kebab",
            "aztec",
            "alban",
            "aztec2",
            "bomb",
            "plant",
            "wasteland",
            "pool",
            "courbet",
            "sea",
            "sunset",
            "creebet",
            "wanderer",
            "graham",
            "match",
            "bust",
            "stage",
            "void",
            "skull_and_roses",
            "wither",
            "fighters",
            "pointer",
            "pigscene",
            "burning_skull",
            "skeleton",
            "earth",
            "wind",
            "water",
            "fire",
            "donkey_kong",
            "baroque",
            "humble",
            "meditative",
            "prairie_ride",
            "unpacked",
            "backyard",
            "bouquet",
            "cavebird",
            "changing",
            "cotan",
            "endboss",
            "fern",
            "finding",
            "lowmist",
            "orb",
            "owlemons",
            "passage",
            "pond",
            "sunflowers",
            "tides",
            "dennis",
        ],
        _ => panic!("Missing vanilla variant order for {subdir}"),
    }
}
