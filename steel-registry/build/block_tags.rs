use rustc_hash::FxHashMap;
use std::{fs, path::Path};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct TagJson {
    values: Vec<String>,
}

#[derive(Deserialize)]
struct TagFile {
    block: FxHashMap<String, Vec<String>>,
}

/// Reads all tag JSON files and returns a map of tag name -> values
fn read_all_tags(tag_dir: &str) -> FxHashMap<String, Vec<String>> {
    let mut tags = FxHashMap::default();

    fn read_directory(dir: &Path, base_path: &Path, tags: &mut FxHashMap<String, Vec<String>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                read_directory(&path, base_path, tags);
            } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
                // Calculate the tag name relative to the base tags directory
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
    read_directory(base_path, base_path, &mut tags);

    tags
}

fn read_all_fabric_tags(tag_file: &str) -> FxHashMap<String, Vec<String>> {
    if fs::exists(tag_file).unwrap_or(false)
        && Path::new(tag_file).is_file()
        && let Ok(content) = fs::read_to_string(tag_file)
    {
        let tag: TagFile = serde_json::from_str(&content)
            .unwrap_or_else(|e| panic!("Failed to parse {}: {}", tag_file, e));
        return tag.block;
    }
    FxHashMap::default()
}

/// Resolves tag references recursively and returns a flattened list of block keys
fn resolve_tag(
    tag_name: &str,
    all_tags: &FxHashMap<String, Vec<String>>,
    resolved_cache: &mut FxHashMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
) -> Vec<String> {
    // Check if already resolved
    if let Some(cached) = resolved_cache.get(tag_name) {
        return cached.clone();
    }

    // Check for circular dependency
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
            // Remove the "minecraft:" prefix if present
            let nested_tag = nested_tag.strip_prefix("minecraft:").unwrap_or(nested_tag);

            // Recursively resolve the nested tag
            let nested_values = resolve_tag(nested_tag, all_tags, resolved_cache, visiting);
            resolved.extend(nested_values);
        } else {
            // Direct block reference - remove "minecraft:" prefix
            let block_key = value.strip_prefix("minecraft:").unwrap_or(value);
            resolved.push(block_key.to_string());
        }
    }

    visiting.pop();

    // Remove duplicates while preserving order
    let mut seen = rustc_hash::FxHashSet::default();
    resolved.retain(|x| seen.insert(x.clone()));

    resolved_cache.insert(tag_name.to_string(), resolved.clone());
    resolved
}

pub(crate) fn build() -> TokenStream {
    println!(
        "cargo:rerun-if-changed=build_assets/builtin_datapacks/minecraft/data/minecraft/tags/block/"
    );

    let tag_dir = "build_assets/builtin_datapacks/minecraft/data/minecraft/tags/block";
    let mut all_tags = read_all_tags(tag_dir);
    all_tags.extend(read_all_fabric_tags("build_assets/tags.json"));

    // Resolve all tags
    let mut resolved_tags: FxHashMap<String, Vec<String>> = FxHashMap::default();
    let mut resolved_cache = FxHashMap::default();

    for tag_name in all_tags.keys() {
        let mut visiting = Vec::new();
        let resolved = resolve_tag(tag_name, &all_tags, &mut resolved_cache, &mut visiting);
        resolved_tags.insert(tag_name.clone(), resolved);
    }

    // Sort tags by name for consistent generation
    let mut sorted_tags: Vec<_> = resolved_tags.into_iter().collect();
    sorted_tags.sort_by(|a, b| a.0.cmp(&b.0));

    let mut stream = TokenStream::new();

    stream.extend(quote! {
        use crate::blocks::BlockRegistry;
        use steel_utils::Identifier;
    });

    let mut register_stream = TokenStream::new();
    let mut static_array = TokenStream::new();
    let mut const_identifier = TokenStream::new();
    // Generate const arrays for each tag
    for (tag_name, blocks) in &sorted_tags {
        let tag_ident_array = Ident::new(
            &format!("{}_TAG_LIST", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );

        let block_strs = blocks.iter().map(|s| s.as_str());

        // No public needed to work, and isn't modifiable
        static_array.extend(quote! {
            static #tag_ident_array: &[&str] = &[#(#block_strs),*];
        });
        let tag_ident = Ident::new(
            &format!("{}_TAG", tag_name.to_shouty_snake_case()),
            Span::call_site(),
        );
        let tag_key = tag_name.clone();

        if let Some(key) = tag_key.strip_prefix("c:") {
            const_identifier.extend(
                quote! { pub const #tag_ident: Identifier = Identifier::new_static("c", #key); },
            );
        } else {
            const_identifier.extend(
                quote! {pub const #tag_ident: Identifier = Identifier::vanilla_static(#tag_key);},
            );
        }
        register_stream.extend(quote! {
            registry.register_tag(
                #tag_ident,
                #tag_ident_array
            );
        });
    }
    stream.extend(quote! {
        #static_array

        #const_identifier
        pub fn register_block_tags(registry: &mut BlockRegistry) {
            #register_stream
        }
    });

    stream
}
