use rustc_hash::{FxHashMap, FxHashSet};
use std::{fs, path::Path};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TagJson {
    pub values: Vec<String>,
}

/// Reads all tag JSON files recursively and returns a map of tag name -> values.
pub fn read_all_tags(tag_dir: &str) -> FxHashMap<String, Vec<String>> {
    let mut tags = FxHashMap::default();

    fn read_directory(dir: &Path, base_path: &Path, tags: &mut FxHashMap<String, Vec<String>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                read_directory(&path, base_path, tags);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let relative_path = path.strip_prefix(base_path).unwrap();
                let tag_name = relative_path
                    .with_extension("")
                    .to_str()
                    .unwrap()
                    .replace('\\', "/");

                let content = fs::read_to_string(&path).unwrap();
                let tag: TagJson = serde_json::from_str(&content)
                    .unwrap_or_else(|e| panic!("Failed to parse {}: {}", tag_name, e));

                tags.insert(tag_name, tag.values);
            }
        }
    }

    let base_path = Path::new(tag_dir);
    if base_path.exists() {
        read_directory(base_path, base_path, &mut tags);
    }

    tags
}

/// Resolves tag references recursively and returns a flattened, deduplicated list of keys.
pub fn resolve_tag(
    tag_name: &str,
    all_tags: &FxHashMap<String, Vec<String>>,
    resolved_cache: &mut FxHashMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
) -> Vec<String> {
    if let Some(cached) = resolved_cache.get(tag_name) {
        return cached.clone();
    }

    if visiting.contains(&tag_name.to_string()) {
        panic!("Circular tag dependency detected: {:?}", visiting);
    }

    visiting.push(tag_name.to_string());

    let values = all_tags
        .get(tag_name)
        .unwrap_or_else(|| panic!("Tag not found: {}", tag_name));

    let mut resolved = Vec::new();

    for value in values {
        if let Some(nested_tag) = value.strip_prefix('#') {
            let nested_tag = nested_tag.strip_prefix("minecraft:").unwrap_or(nested_tag);
            let nested_values = resolve_tag(nested_tag, all_tags, resolved_cache, visiting);
            resolved.extend(nested_values);
        } else {
            let key = value.strip_prefix("minecraft:").unwrap_or(value);
            resolved.push(key.to_string());
        }
    }

    visiting.pop();

    let mut seen = FxHashSet::default();
    resolved.retain(|x| seen.insert(x.clone()));

    resolved_cache.insert(tag_name.to_string(), resolved.clone());
    resolved
}

/// Resolves all tags and returns them sorted by name.
pub fn resolve_all_tags(all_tags: &FxHashMap<String, Vec<String>>) -> Vec<(String, Vec<String>)> {
    let mut resolved_tags: FxHashMap<String, Vec<String>> = FxHashMap::default();
    let mut resolved_cache = FxHashMap::default();

    for tag_name in all_tags.keys() {
        let mut visiting = Vec::new();
        let resolved = resolve_tag(tag_name, all_tags, &mut resolved_cache, &mut visiting);
        resolved_tags.insert(tag_name.clone(), resolved);
    }

    let mut sorted: Vec<_> = resolved_tags.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    sorted
}

/// Builds a complete tag module for a vanilla-only registry.
///
/// Generates: static tag arrays, `pub const` tag identifiers, and a register function.
///
/// - `tag_subdir`: directory under `tags/` (e.g., `"damage_type"`)
/// - `registry_module`: crate module name (e.g., `"damage_type"`)
/// - `registry_type`: type name (e.g., `"DamageTypeRegistry"`)
/// - `register_fn`: function name (e.g., `"register_damage_type_tags"`)
pub fn build_simple_tags(
    tag_subdir: &str,
    registry_module: &str,
    registry_type: &str,
    register_fn: &str,
) -> TokenStream {
    let tag_dir =
        format!("build_assets/builtin_datapacks/minecraft/data/minecraft/tags/{tag_subdir}");
    println!("cargo:rerun-if-changed={tag_dir}");

    let all_tags = read_all_tags(&tag_dir);
    let sorted_tags = resolve_all_tags(&all_tags);

    let registry_module_ident = Ident::new(registry_module, Span::call_site());
    let registry_type_ident = Ident::new(registry_type, Span::call_site());
    let register_fn_ident = Ident::new(register_fn, Span::call_site());

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::#registry_module_ident::#registry_type_ident;
        use crate::TaggedRegistryExt;
        use steel_utils::Identifier;
    });

    let mut static_arrays = TokenStream::new();
    let mut const_identifiers = TokenStream::new();
    let mut register_stream = TokenStream::new();

    for (tag_name, entries) in &sorted_tags {
        let tag_list_ident = Ident::new(
            &format!("{}_TAG_LIST", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let tag_ident = Ident::new(
            &format!("{}_TAG", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );

        let entry_strs = entries.iter().map(|s| s.as_str());
        let tag_key = tag_name.as_str();

        static_arrays.extend(quote! {
            static #tag_list_ident: &[&str] = &[#(#entry_strs),*];
        });

        const_identifiers.extend(quote! {
            pub const #tag_ident: Identifier = Identifier::vanilla_static(#tag_key);
        });

        register_stream.extend(quote! {
            registry.register_tag(#tag_ident, #tag_list_ident);
        });
    }

    stream.extend(quote! {
        #static_arrays

        #const_identifiers

        pub fn #register_fn_ident(registry: &mut #registry_type_ident) {
            #register_stream
        }
    });

    stream
}
