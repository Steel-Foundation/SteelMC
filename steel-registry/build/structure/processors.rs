//! Build-time codegen for vanilla structure processor-list registry entries.

use std::fs;

use crate::generator_functions::{
    generate_static_identifier as generate_identifier, generate_static_identifier_from_str,
    registry_entry_ident, resource_name, sorted_json_files,
};
use proc_macro2::TokenStream;
use quote::quote;
use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use steel_utils::{Identifier, value_providers::IntProvider};

#[expect(
    dead_code,
    reason = "imported processor data contains variants not emitted by current vanilla assets"
)]
#[path = "../../src/structure/processor/data.rs"]
mod structure_processor_data;

use crate::shared_structs::BlockStateData;
use structure_processor_data::{
    PosRuleTestData, ProcessorRuleData, RuleBlockEntityModifierData, StructureProcessorAxis,
    StructureProcessorHeightmap, StructureProcessorKind, StructureProcessorListData,
    StructureRuleTestData,
};

fn generate_registry_key(registry_id: &str) -> TokenStream {
    generate_static_identifier_from_str(registry_id, "structure processor-list registry")
}

fn generate_option<T>(value: &Option<T>, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    if let Some(value) = value {
        let value = f(value);
        quote! { Some(#value) }
    } else {
        quote! { None }
    }
}

fn generate_vec<T>(values: &[T], f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let values = values.iter().map(f);
    quote! { vec![#(#values),*] }
}

fn generate_box<T>(value: &T, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let value = f(value);
    quote! { Box::new(#value) }
}

fn generate_block_state_data(data: &BlockStateData) -> TokenStream {
    let name = generate_identifier(&data.name);
    let properties = if data.properties.is_empty() {
        quote! { std::collections::BTreeMap::new() }
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            quote! { (#key.to_owned(), #value.to_owned()) }
        });
        quote! { std::collections::BTreeMap::from([#(#entries),*]) }
    };

    quote! {
        crate::shared_structs::BlockStateData {
            name: #name,
            properties: #properties,
        }
    }
}

fn generate_int_provider(provider: &IntProvider) -> TokenStream {
    match provider {
        IntProvider::Constant(value) => quote! { IntProvider::Constant(#value) },
        IntProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::BiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => quote! {
            IntProvider::VeryBiasedToBottom {
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
                inner: #inner,
            }
        },
        IntProvider::Trapezoid { min, max, plateau } => quote! {
            IntProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        IntProvider::ClampedNormal {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        } => quote! {
            IntProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min_inclusive: #min_inclusive,
                max_inclusive: #max_inclusive,
            }
        },
        IntProvider::Clamped {
            source,
            min_inclusive,
            max_inclusive,
        } => {
            let source = generate_box(source.as_ref(), generate_int_provider);
            quote! {
                IntProvider::Clamped {
                    source: #source,
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        IntProvider::WeightedList { distribution } => {
            let distribution = generate_vec(distribution, |entry| {
                let data = generate_int_provider(&entry.data);
                let weight = entry.weight;
                quote! { steel_utils::value_providers::WeightedIntProvider { data: #data, weight: #weight } }
            });
            quote! { IntProvider::WeightedList { distribution: #distribution } }
        }
    }
}

fn generate_processor_axis(axis: StructureProcessorAxis) -> TokenStream {
    match axis {
        StructureProcessorAxis::X => quote! { StructureProcessorAxis::X },
        StructureProcessorAxis::Y => quote! { StructureProcessorAxis::Y },
        StructureProcessorAxis::Z => quote! { StructureProcessorAxis::Z },
    }
}

fn generate_pos_rule_test(data: &PosRuleTestData) -> TokenStream {
    match data {
        PosRuleTestData::AlwaysTrue => quote! { PosRuleTestData::AlwaysTrue },
        PosRuleTestData::AxisAlignedLinearPos {
            axis,
            min_chance,
            max_chance,
            min_dist,
            max_dist,
        } => {
            let axis = generate_processor_axis(*axis);
            quote! {
                PosRuleTestData::AxisAlignedLinearPos {
                    axis: #axis,
                    min_chance: #min_chance,
                    max_chance: #max_chance,
                    min_dist: #min_dist,
                    max_dist: #max_dist,
                }
            }
        }
    }
}

fn generate_rule_test(data: &StructureRuleTestData) -> TokenStream {
    match data {
        StructureRuleTestData::AlwaysTrue => quote! { StructureRuleTestData::AlwaysTrue },
        StructureRuleTestData::BlockMatch { block } => {
            let block = generate_identifier(block);
            quote! { StructureRuleTestData::BlockMatch { block: #block } }
        }
        StructureRuleTestData::RandomBlockMatch { block, probability } => {
            let block = generate_identifier(block);
            quote! {
                StructureRuleTestData::RandomBlockMatch {
                    block: #block,
                    probability: #probability,
                }
            }
        }
        StructureRuleTestData::TagMatch { tag } => {
            let tag = generate_identifier(tag);
            quote! { StructureRuleTestData::TagMatch { tag: #tag } }
        }
        StructureRuleTestData::BlockStateMatch { block_state } => {
            let block_state = generate_block_state_data(block_state);
            quote! { StructureRuleTestData::BlockStateMatch { block_state: #block_state } }
        }
        StructureRuleTestData::RandomBlockStateMatch {
            block_state,
            probability,
        } => {
            let block_state = generate_block_state_data(block_state);
            quote! {
                StructureRuleTestData::RandomBlockStateMatch {
                    block_state: #block_state,
                    probability: #probability,
                }
            }
        }
    }
}

fn generate_block_entity_modifier(data: &RuleBlockEntityModifierData) -> TokenStream {
    match data {
        RuleBlockEntityModifierData::Passthrough => {
            quote! { RuleBlockEntityModifierData::Passthrough }
        }
        RuleBlockEntityModifierData::AppendLoot { loot_table } => {
            let loot_table = generate_identifier(loot_table);
            quote! { RuleBlockEntityModifierData::AppendLoot { loot_table: #loot_table } }
        }
        RuleBlockEntityModifierData::AppendStatic { data } => {
            let data = generate_nbt_compound(data);
            quote! { RuleBlockEntityModifierData::AppendStatic { data: #data } }
        }
    }
}

fn generate_nbt_compound(compound: &NbtCompound) -> TokenStream {
    let entries = compound.iter().map(|(key, value)| {
        let key = key.to_string();
        let value = generate_nbt_tag(value);
        quote! { (#key.into(), #value) }
    });
    quote! { NbtCompound::from_values(vec![#(#entries),*]) }
}

fn generate_nbt_list(list: &NbtList) -> TokenStream {
    match list {
        NbtList::Empty => quote! { NbtList::Empty },
        NbtList::Byte(values) => quote! { NbtList::Byte(vec![#(#values),*]) },
        NbtList::Short(values) => quote! { NbtList::Short(vec![#(#values),*]) },
        NbtList::Int(values) => quote! { NbtList::Int(vec![#(#values),*]) },
        NbtList::Long(values) => quote! { NbtList::Long(vec![#(#values),*]) },
        NbtList::Float(values) => quote! { NbtList::Float(vec![#(#values),*]) },
        NbtList::Double(values) => quote! { NbtList::Double(vec![#(#values),*]) },
        NbtList::ByteArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { NbtList::ByteArray(vec![#(#values),*]) }
        }
        NbtList::String(values) => {
            let values = values
                .iter()
                .map(|value| value.as_str().to_str().into_owned());
            quote! { NbtList::String(vec![#(#values.into()),*]) }
        }
        NbtList::List(values) => {
            let values = values.iter().map(generate_nbt_list);
            quote! { NbtList::List(vec![#(#values),*]) }
        }
        NbtList::Compound(values) => {
            let values = values.iter().map(generate_nbt_compound);
            quote! { NbtList::Compound(vec![#(#values),*]) }
        }
        NbtList::IntArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { NbtList::IntArray(vec![#(#values),*]) }
        }
        NbtList::LongArray(values) => {
            let values = values.iter().map(|value| quote! { vec![#(#value),*] });
            quote! { NbtList::LongArray(vec![#(#values),*]) }
        }
    }
}

fn generate_nbt_tag(tag: &NbtTag) -> TokenStream {
    match tag {
        NbtTag::Byte(value) => quote! { NbtTag::Byte(#value) },
        NbtTag::Short(value) => quote! { NbtTag::Short(#value) },
        NbtTag::Int(value) => quote! { NbtTag::Int(#value) },
        NbtTag::Long(value) => quote! { NbtTag::Long(#value) },
        NbtTag::Float(value) => quote! { NbtTag::Float(#value) },
        NbtTag::Double(value) => quote! { NbtTag::Double(#value) },
        NbtTag::ByteArray(value) => quote! { NbtTag::ByteArray(vec![#(#value),*]) },
        NbtTag::String(value) => {
            let value = value.as_str().to_str().into_owned();
            quote! { NbtTag::String(#value.into()) }
        }
        NbtTag::List(value) => {
            let value = generate_nbt_list(value);
            quote! { NbtTag::List(#value) }
        }
        NbtTag::Compound(value) => {
            let value = generate_nbt_compound(value);
            quote! { NbtTag::Compound(#value) }
        }
        NbtTag::IntArray(value) => quote! { NbtTag::IntArray(vec![#(#value),*]) },
        NbtTag::LongArray(value) => quote! { NbtTag::LongArray(vec![#(#value),*]) },
    }
}

fn generate_processor_rule(data: &ProcessorRuleData) -> TokenStream {
    let input_predicate = generate_rule_test(&data.input_predicate);
    let location_predicate = generate_rule_test(&data.location_predicate);
    let position_predicate = generate_pos_rule_test(&data.position_predicate);
    let output_state = generate_block_state_data(&data.output_state);
    let block_entity_modifier = generate_block_entity_modifier(&data.block_entity_modifier);

    quote! {
        ProcessorRuleData {
            input_predicate: #input_predicate,
            location_predicate: #location_predicate,
            position_predicate: #position_predicate,
            output_state: #output_state,
            block_entity_modifier: #block_entity_modifier,
        }
    }
}

fn generate_processor_heightmap(heightmap: StructureProcessorHeightmap) -> TokenStream {
    match heightmap {
        StructureProcessorHeightmap::WorldSurface => {
            quote! { StructureProcessorHeightmap::WorldSurface }
        }
        StructureProcessorHeightmap::MotionBlocking => {
            quote! { StructureProcessorHeightmap::MotionBlocking }
        }
        StructureProcessorHeightmap::MotionBlockingNoLeaves => {
            quote! { StructureProcessorHeightmap::MotionBlockingNoLeaves }
        }
        StructureProcessorHeightmap::OceanFloor => {
            quote! { StructureProcessorHeightmap::OceanFloor }
        }
        StructureProcessorHeightmap::WorldSurfaceWg => {
            quote! { StructureProcessorHeightmap::WorldSurfaceWg }
        }
        StructureProcessorHeightmap::OceanFloorWg => {
            quote! { StructureProcessorHeightmap::OceanFloorWg }
        }
    }
}

pub(crate) fn generate_processor_kind(data: &StructureProcessorKind) -> TokenStream {
    match data {
        StructureProcessorKind::BlockRot {
            rottable_blocks,
            integrity,
        } => {
            let rottable_blocks = generate_option(rottable_blocks, generate_identifier);
            quote! {
                StructureProcessorKind::BlockRot {
                    rottable_blocks: #rottable_blocks,
                    integrity: #integrity,
                }
            }
        }
        StructureProcessorKind::ProtectedBlocks { cannot_replace } => {
            let cannot_replace = generate_identifier(cannot_replace);
            quote! { StructureProcessorKind::ProtectedBlocks { cannot_replace: #cannot_replace } }
        }
        StructureProcessorKind::Rule { rules } => {
            let rules = generate_vec(rules, generate_processor_rule);
            quote! { StructureProcessorKind::Rule { rules: #rules } }
        }
        StructureProcessorKind::BlockAge { mossiness } => {
            quote! { StructureProcessorKind::BlockAge { mossiness: #mossiness } }
        }
        StructureProcessorKind::LavaSubmergedBlock => {
            quote! { StructureProcessorKind::LavaSubmergedBlock }
        }
        StructureProcessorKind::JigsawReplacement => {
            quote! { StructureProcessorKind::JigsawReplacement }
        }
        StructureProcessorKind::BlackstoneReplace => {
            quote! { StructureProcessorKind::BlackstoneReplace }
        }
        StructureProcessorKind::BlockIgnore { blocks } => {
            let blocks = generate_vec(blocks, generate_block_state_data);
            quote! { StructureProcessorKind::BlockIgnore { blocks: #blocks } }
        }
        StructureProcessorKind::Gravity { heightmap, offset } => {
            let heightmap = generate_processor_heightmap(*heightmap);
            quote! {
                StructureProcessorKind::Gravity {
                    heightmap: #heightmap,
                    offset: #offset,
                }
            }
        }
        StructureProcessorKind::Capped { delegate, limit } => {
            let delegate = generate_box(delegate.as_ref(), generate_processor_kind);
            let limit = generate_int_provider(limit);
            quote! { StructureProcessorKind::Capped { delegate: #delegate, limit: #limit } }
        }
    }
}

fn generate_processor_list_data(data: &StructureProcessorListData) -> TokenStream {
    let processors = generate_vec(&data.processors, generate_processor_kind);
    quote! { StructureProcessorListData { processors: #processors } }
}

pub(crate) fn build() -> TokenStream {
    let dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/processor_list";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.as_path();
        let content =
            fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        let data = serde_json::from_str::<StructureProcessorListData>(&content)
            .unwrap_or_else(|err| panic!("failed to parse structure processor list {name}: {err}"));
        entries.push((name, data));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::shared_structs::BlockStateData;
        use crate::structure_processor::{
            PosRuleTestData, ProcessorRuleData, RuleBlockEntityModifierData, StructureProcessorAxis,
            StructureProcessorKind, StructureProcessorList, StructureProcessorListData,
            StructureProcessorListRegistry, StructureRuleTestData,
        };
        use steel_utils::Identifier;
        use steel_utils::value_providers::IntProvider;
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (registry_id, data) in &entries {
        let ident = registry_entry_ident(registry_id);
        let key = generate_registry_key(registry_id);
        let data = generate_processor_list_data(data);
        stream.extend(quote! {
            pub static #ident: LazyLock<StructureProcessorList> = LazyLock::new(|| {
                StructureProcessorList {
                    key: #key,
                    data: #data,
                    id: OnceLock::new(),
                }
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_structure_processor_lists(registry: &mut StructureProcessorListRegistry) {
            #register
        }
    });

    stream
}
