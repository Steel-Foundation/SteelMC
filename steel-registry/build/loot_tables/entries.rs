use super::{
    LootEntryJson, LootPoolJson, LootTableValueJson, TokenStream, generate_condition,
    generate_function, generate_number_provider, generate_static_identifier_from_str, quote,
};

pub(super) fn generate_entry(entry: &LootEntryJson) -> TokenStream {
    let conditions: Vec<TokenStream> = entry.conditions.iter().map(generate_condition).collect();
    let functions: Vec<TokenStream> = entry.functions.iter().map(generate_function).collect();
    let entry_type = if entry.entry_type.contains(':') {
        entry.entry_type.clone()
    } else {
        format!("minecraft:{}", entry.entry_type)
    };

    match entry_type.as_str() {
        "minecraft:item" => {
            let name = generate_static_identifier_from_str(
                entry.name.as_deref().unwrap_or("minecraft:air"),
                "loot",
            );
            let weight = entry.weight;
            let quality = entry.quality;
            quote! {
                LootEntry::Item {
                    name: #name,
                    weight: #weight,
                    quality: #quality,
                    conditions: &[#(#conditions),*],
                    functions: &[#(#functions),*],
                }
            }
        }
        "minecraft:loot_table" => {
            let weight = entry.weight;
            let quality = entry.quality;

            // Check if it's a string reference or inline loot table
            if let Some(name) = entry.name.as_deref() {
                let name = generate_static_identifier_from_str(name, "loot");
                quote! {
                    LootEntry::LootTableRef {
                        name: #name,
                        weight: #weight,
                        quality: #quality,
                        conditions: &[#(#conditions),*],
                        functions: &[#(#functions),*],
                    }
                }
            } else if let Some(value) = &entry.value {
                match value {
                    LootTableValueJson::Reference(s) => {
                        let name = generate_static_identifier_from_str(s, "loot");
                        quote! {
                            LootEntry::LootTableRef {
                                name: #name,
                                weight: #weight,
                                quality: #quality,
                                conditions: &[#(#conditions),*],
                                functions: &[#(#functions),*],
                            }
                        }
                    }
                    LootTableValueJson::Inline(inline) => {
                        let inline_pools: Vec<TokenStream> =
                            inline.pools.iter().map(generate_pool).collect();
                        let inline_functions: Vec<TokenStream> =
                            inline.functions.iter().map(generate_function).collect();

                        quote! {
                            LootEntry::InlineLootTable {
                                pools: &[#(#inline_pools),*],
                                table_functions: &[#(#inline_functions),*],
                                weight: #weight,
                                quality: #quality,
                                conditions: &[#(#conditions),*],
                                functions: &[#(#functions),*],
                            }
                        }
                    }
                }
            } else {
                quote! {
                    LootEntry::LootTableRef {
                        name: Identifier::vanilla_static("empty"),
                        weight: #weight,
                        quality: #quality,
                        conditions: &[#(#conditions),*],
                        functions: &[#(#functions),*],
                    }
                }
            }
        }
        "minecraft:tag" => {
            let name = generate_static_identifier_from_str(
                entry.name.as_deref().unwrap_or("minecraft:empty"),
                "loot",
            );
            let expand = entry.expand;
            let weight = entry.weight;
            let quality = entry.quality;
            quote! {
                LootEntry::Tag {
                    name: #name,
                    expand: #expand,
                    weight: #weight,
                    quality: #quality,
                    conditions: &[#(#conditions),*],
                    functions: &[#(#functions),*],
                }
            }
        }
        "minecraft:alternatives" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Alternatives {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:group" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Group {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:sequence" => {
            let children: Vec<TokenStream> = entry.children.iter().map(generate_entry).collect();
            quote! {
                LootEntry::Sequence {
                    children: &[#(#children),*],
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:empty" => {
            let weight = entry.weight;
            quote! {
                LootEntry::Empty {
                    weight: #weight,
                    conditions: &[#(#conditions),*],
                }
            }
        }
        "minecraft:dynamic" => {
            let name = entry.name.as_deref().unwrap_or("contents");
            let name = name.strip_prefix("minecraft:").unwrap_or(name);
            quote! {
                LootEntry::Dynamic {
                    name: Identifier::vanilla_static(#name),
                    conditions: &[#(#conditions),*],
                }
            }
        }
        other => {
            panic!("Unknown loot entry type: {other}");
        }
    }
}

pub(super) fn generate_pool(pool: &LootPoolJson) -> TokenStream {
    let rolls = generate_number_provider(&pool.rolls);
    let bonus_rolls = generate_number_provider(&pool.bonus_rolls);
    let entries: Vec<TokenStream> = pool.entries.iter().map(generate_entry).collect();
    let conditions: Vec<TokenStream> = pool.conditions.iter().map(generate_condition).collect();
    let functions: Vec<TokenStream> = pool.functions.iter().map(generate_function).collect();

    quote! {
        LootPool {
            rolls: #rolls,
            bonus_rolls: #bonus_rolls,
            entries: &[#(#entries),*],
            conditions: &[#(#conditions),*],
            functions: &[#(#functions),*],
        }
    }
}
