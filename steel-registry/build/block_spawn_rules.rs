use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use serde::Deserialize;

use super::blocks::StateBooleanData;

#[derive(Deserialize)]
#[serde(untagged)]
enum SpawnRule {
    Always(bool),
    Allowed(Vec<String>),
}

#[derive(Deserialize)]
pub(super) struct BlockSpawnRules {
    overrides: BTreeMap<String, SpawnRule>,
    signal_sources: BTreeSet<String>,
    land_pathfinding: BTreeMap<String, StateBooleanData>,
}

impl BlockSpawnRules {
    pub(super) fn read() -> Self {
        const PATH: &str = "build_assets/block_spawn_rules.json";
        println!("cargo:rerun-if-changed={PATH}");
        let data = fs::read_to_string(PATH)
            .unwrap_or_else(|error| panic!("Failed to read {PATH}: {error}"));
        serde_json::from_str(&data)
            .unwrap_or_else(|error| panic!("Failed to parse {PATH}: {error}"))
    }

    pub(super) fn take_tokens(&mut self, block_name: &str) -> TokenStream {
        let key = format!("minecraft:{block_name}");
        let rule = self.overrides.remove(&key);
        let signal_source = self.signal_sources.remove(&key);
        let land_pathfinding = self.land_pathfinding.remove(&key);
        if rule.is_none() && !signal_source && land_pathfinding.is_none() {
            return TokenStream::new();
        }

        let rule = match rule {
            None => quote! { BlockSpawnRule::Default },
            Some(SpawnRule::Always(true)) => quote! { BlockSpawnRule::Always },
            Some(SpawnRule::Always(false)) => quote! { BlockSpawnRule::Allowed(&[]) },
            Some(SpawnRule::Allowed(types)) => {
                let types = types.into_iter().map(|key| {
                    let path = key.strip_prefix("minecraft:").unwrap_or_else(|| {
                        panic!("Non-vanilla entity type {key} in block spawn rules")
                    });

                    Ident::new(&path.to_shouty_snake_case(), Span::call_site())
                });

                quote! { BlockSpawnRule::Allowed(&[ #(&vanilla_entities::#types),* ]) }
            }
        };

        let land_pathfinding = land_pathfinding.map_or_else(
            || quote! { None },
            |data| {
                let default = data.default;
                let overwrites = data.overwrites.into_iter().map(|overwrite| {
                    let offset = overwrite.offset;
                    let value = overwrite.value;

                    quote! { StateBooleanOverwrite::new(#offset, #value) }
                });

                quote! { Some(StateBooleanData::new(#default, &[#(#overwrites),*])) }
            },
        );
        quote! { .with_spawn_data(#rule, #signal_source, #land_pathfinding) }
    }

    pub(super) fn assert_consumed(&self) {
        assert!(
            self.overrides.is_empty()
                && self.signal_sources.is_empty()
                && self.land_pathfinding.is_empty(),
            "Unknown blocks in spawn rules: {:?}, {:?}, {:?}",
            self.overrides.keys(),
            self.signal_sources,
            self.land_pathfinding.keys(),
        );
    }
}
