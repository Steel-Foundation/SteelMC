//! Build-time NBT codegen for the `worldgen/block_state_provider` registry
//! sync packet. Mirrors `BlockStateProvider.DIRECT_CODEC`'s field names —
//! the same names already used by this module's JSON [`Deserialize`] impls,
//! since vanilla's codecs are format-agnostic.

use super::{
    BlockHolderSet, BlockPredicate, BlockStateData, BlockStateProviderKind, FeatureNoiseParameters,
    Identifier, IntProvider, TokenStream, VerticalAnchor, WeightedIntProvider, quote,
};

pub(super) fn generate_identifier_nbt(identifier: &Identifier) -> TokenStream {
    let id = identifier.to_string();
    quote! { NbtTag::String(#id.into()) }
}

/// Builds an `NbtTag::List`, using `NbtList::Empty` for an empty source —
/// `NbtList::from(vec![])` can't infer its element type.
fn generate_nbt_list(items: impl Iterator<Item = TokenStream>) -> TokenStream {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        quote! { NbtTag::List(NbtList::Empty) }
    } else {
        quote! { NbtTag::List(NbtList::from(vec![#(#items),*])) }
    }
}

pub(super) fn generate_block_state_data_nbt(data: &BlockStateData) -> TokenStream {
    let id = generate_identifier_nbt(&data.name);
    if data.properties.is_empty() {
        quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("id", #id);
            NbtTag::Compound(compound)
        }}
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            quote! { properties.insert(#key, #value); }
        });
        quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("id", #id);
            let mut properties = NbtCompound::new();
            #(#entries)*
            compound.insert("properties", NbtTag::Compound(properties));
            NbtTag::Compound(compound)
        }}
    }
}

pub(super) fn generate_block_holder_set_nbt(set: &BlockHolderSet) -> TokenStream {
    match set {
        BlockHolderSet::Tag(tag) => {
            let tag = format!("#{tag}");
            quote! { NbtTag::String(#tag.into()) }
        }
        BlockHolderSet::Entries(entries) => {
            generate_nbt_list(entries.iter().map(generate_identifier_nbt))
        }
    }
}

pub(super) fn generate_weighted_int_provider_nbt(provider: &WeightedIntProvider) -> TokenStream {
    let data = generate_int_provider_nbt(&provider.data);
    let weight = provider.weight;
    quote! {{
        let mut compound = NbtCompound::new();
        compound.insert("data", #data);
        compound.insert("weight", #weight);
        NbtTag::Compound(compound)
    }}
}

pub(super) fn generate_int_provider_nbt(provider: &IntProvider) -> TokenStream {
    match provider {
        IntProvider::Constant(value) => quote! { NbtTag::Int(#value) },
        IntProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:uniform");
            compound.insert("min_inclusive", #min_inclusive);
            compound.insert("max_inclusive", #max_inclusive);
            NbtTag::Compound(compound)
        }},
        IntProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
        } => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:biased_to_bottom");
            compound.insert("min_inclusive", #min_inclusive);
            compound.insert("max_inclusive", #max_inclusive);
            NbtTag::Compound(compound)
        }},
        IntProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:very_biased_to_bottom");
            compound.insert("min_inclusive", #min_inclusive);
            compound.insert("max_inclusive", #max_inclusive);
            compound.insert("inner", #inner);
            NbtTag::Compound(compound)
        }},
        IntProvider::Trapezoid { min, max, plateau } => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:trapezoid");
            compound.insert("min", #min);
            compound.insert("max", #max);
            compound.insert("plateau", #plateau);
            NbtTag::Compound(compound)
        }},
        IntProvider::ClampedNormal {
            mean,
            deviation,
            min_inclusive,
            max_inclusive,
        } => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:clamped_normal");
            compound.insert("mean", #mean);
            compound.insert("deviation", #deviation);
            compound.insert("min_inclusive", #min_inclusive);
            compound.insert("max_inclusive", #max_inclusive);
            NbtTag::Compound(compound)
        }},
        IntProvider::Clamped {
            source,
            min_inclusive,
            max_inclusive,
        } => {
            let source = generate_int_provider_nbt(source);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:clamped");
                compound.insert("source", #source);
                compound.insert("min_inclusive", #min_inclusive);
                compound.insert("max_inclusive", #max_inclusive);
                NbtTag::Compound(compound)
            }}
        }
        IntProvider::WeightedList { distribution } => {
            let distribution =
                generate_nbt_list(distribution.iter().map(generate_weighted_int_provider_nbt));
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:weighted_list");
                compound.insert("distribution", #distribution);
                NbtTag::Compound(compound)
            }}
        }
    }
}

pub(super) fn generate_vertical_anchor_nbt(anchor: VerticalAnchor) -> TokenStream {
    let (key, value) = match anchor {
        VerticalAnchor::Absolute(value) => ("absolute", value),
        VerticalAnchor::AboveBottom(value) => ("above_bottom", value),
        VerticalAnchor::BelowTop(value) => ("below_top", value),
        VerticalAnchor::RelativeToSeaLevel(value) => ("relative_to_sea_level", value),
    };
    quote! {{
        let mut compound = NbtCompound::new();
        compound.insert(#key, #value);
        NbtTag::Compound(compound)
    }}
}

pub(super) fn generate_offset_nbt(offset: &[i32; 3]) -> TokenStream {
    let [x, y, z] = *offset;
    quote! { NbtTag::IntArray(vec![#x, #y, #z]) }
}

pub(super) fn generate_block_predicate_nbt(predicate: &BlockPredicate) -> TokenStream {
    match predicate {
        BlockPredicate::True => quote! {{
            let mut compound = NbtCompound::new();
            compound.insert("type", "minecraft:true");
            NbtTag::Compound(compound)
        }},
        BlockPredicate::AllOf { predicates } => {
            let predicates = generate_nbt_list(predicates.iter().map(generate_block_predicate_nbt));
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:all_of");
                compound.insert("predicates", #predicates);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::AnyOf { predicates } => {
            let predicates = generate_nbt_list(predicates.iter().map(generate_block_predicate_nbt));
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:any_of");
                compound.insert("predicates", #predicates);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::Not { predicate } => {
            let predicate = generate_block_predicate_nbt(predicate);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:not");
                compound.insert("predicate", #predicate);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::MatchingBlockTag { tag, offset } => {
            let tag = tag.to_string();
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:matching_block_tag");
                compound.insert("tag", #tag);
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::MatchingBlocks { blocks, offset } => {
            let blocks = generate_nbt_list(blocks.0.iter().map(generate_identifier_nbt));
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:matching_blocks");
                compound.insert("blocks", #blocks);
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::MatchingFluids { fluids, offset } => {
            let fluids = generate_nbt_list(fluids.0.iter().map(generate_identifier_nbt));
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:matching_fluids");
                compound.insert("fluids", #fluids);
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::Solid { offset } => {
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:solid");
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::WouldSurvive { state, offset } => {
            let state = generate_block_state_data_nbt(state);
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:would_survive");
                compound.insert("state", #state);
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::Replaceable { offset } => {
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:replaceable");
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::HasSturdyFace { direction, offset } => {
            let direction = direction_name(*direction);
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:has_sturdy_face");
                compound.insert("direction", #direction);
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::InsideWorldBounds { offset } => {
            let offset = generate_offset_nbt(offset);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:inside_world_bounds");
                compound.insert("offset", #offset);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::HeightRange {
            min_inclusive,
            max_inclusive,
        } => {
            let min_inclusive = generate_vertical_anchor_nbt(*min_inclusive);
            let max_inclusive = generate_vertical_anchor_nbt(*max_inclusive);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:height_range");
                compound.insert("min_inclusive", #min_inclusive);
                compound.insert("max_inclusive", #max_inclusive);
                NbtTag::Compound(compound)
            }}
        }
        BlockPredicate::VolumeMatch { min, max, matches } => {
            let min = generate_offset_nbt(min);
            let max = generate_offset_nbt(max);
            let matches = generate_block_predicate_nbt(matches);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:volume_match");
                compound.insert("min", #min);
                compound.insert("max", #max);
                compound.insert("match", #matches);
                NbtTag::Compound(compound)
            }}
        }
    }
}

fn direction_name(direction: steel_utils::Direction) -> &'static str {
    match direction {
        steel_utils::Direction::Down => "down",
        steel_utils::Direction::Up => "up",
        steel_utils::Direction::North => "north",
        steel_utils::Direction::South => "south",
        steel_utils::Direction::West => "west",
        steel_utils::Direction::East => "east",
    }
}

pub(super) fn generate_feature_noise_parameters_nbt(
    parameters: &FeatureNoiseParameters,
) -> TokenStream {
    let base_amplitude = parameters.base_amplitude;
    let base_octave = parameters.base_octave;
    let octave_count = parameters.octave_count;
    let normalize = i8::from(parameters.normalize);
    let amplitude_modifiers = generate_nbt_list(
        parameters
            .amplitude_modifiers
            .iter()
            .map(|value| quote! { NbtTag::Double(#value) }),
    );
    quote! {{
        let mut compound = NbtCompound::new();
        compound.insert("base_amplitude", #base_amplitude);
        compound.insert("base_octave", #base_octave);
        compound.insert("octave_count", #octave_count);
        compound.insert("normalize", NbtTag::Byte(#normalize));
        compound.insert("amplitude_modifiers", #amplitude_modifiers);
        NbtTag::Compound(compound)
    }}
}

pub(super) fn generate_block_state_provider_kind_nbt(
    provider: &BlockStateProviderKind,
) -> TokenStream {
    match provider {
        BlockStateProviderKind::Reference(id) => generate_identifier_nbt(id),
        BlockStateProviderKind::Simple { state } => {
            let state = generate_block_state_data_nbt(state);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:simple");
                compound.insert("state", #state);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::Weighted { entries } => {
            let entries = generate_nbt_list(entries.iter().map(|entry| {
                let data = generate_block_state_data_nbt(&entry.data);
                let weight = entry.weight;
                quote! {{
                    let mut compound = NbtCompound::new();
                    compound.insert("data", #data);
                    compound.insert("weight", #weight);
                    NbtTag::Compound(compound)
                }}
            }));
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:weighted");
                compound.insert("entries", #entries);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::RotatedBlock { state, direction } => {
            let state = generate_block_state_provider_kind_nbt(state);
            let mut fields = vec![quote! { compound.insert("type", "minecraft:rotated"); }];
            fields.push(quote! { compound.insert("state", #state); });
            if let Some(direction) = direction {
                let direction = direction_name(*direction);
                fields.push(quote! { compound.insert("direction", #direction); });
            }
            quote! {{
                let mut compound = NbtCompound::new();
                #(#fields)*
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::RandomizedInt {
            property,
            source,
            values,
        } => {
            let source = generate_block_state_provider_kind_nbt(source);
            let values = generate_int_provider_nbt(values);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:randomized_int");
                compound.insert("property", #property);
                compound.insert("source", #source);
                compound.insert("values", #values);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::RuleBased { fallback, rules } => {
            let rules = generate_nbt_list(rules.iter().map(|rule| {
                let if_true = generate_block_predicate_nbt(&rule.if_true);
                let then = generate_block_state_provider_kind_nbt(&rule.then);
                quote! {{
                    let mut compound = NbtCompound::new();
                    compound.insert("if_true", #if_true);
                    compound.insert("then", #then);
                    NbtTag::Compound(compound)
                }}
            }));
            let mut fields = vec![quote! { compound.insert("type", "minecraft:rule_based"); }];
            if let Some(fallback) = fallback {
                let fallback = generate_block_state_provider_kind_nbt(fallback);
                fields.push(quote! { compound.insert("fallback", #fallback); });
            }
            fields.push(quote! {
                compound.insert("rules", #rules);
            });
            quote! {{
                let mut compound = NbtCompound::new();
                #(#fields)*
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::Noise(provider) => {
            let noise = generate_feature_noise_parameters_nbt(&provider.noise);
            let scale = provider.scale;
            let seed = provider.seed;
            let states =
                generate_nbt_list(provider.states.iter().map(generate_block_state_data_nbt));
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:noise");
                compound.insert("noise", #noise);
                compound.insert("scale", #scale);
                compound.insert("seed", #seed);
                compound.insert("states", #states);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::NoiseThreshold(provider) => {
            let noise = generate_feature_noise_parameters_nbt(&provider.noise);
            let scale = provider.scale;
            let seed = provider.seed;
            let threshold = provider.threshold;
            let high_chance = provider.high_chance;
            let default_state = generate_block_state_data_nbt(&provider.default_state);
            let low_states = generate_nbt_list(
                provider
                    .low_states
                    .iter()
                    .map(generate_block_state_data_nbt),
            );
            let high_states = generate_nbt_list(
                provider
                    .high_states
                    .iter()
                    .map(generate_block_state_data_nbt),
            );
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:noise_threshold");
                compound.insert("noise", #noise);
                compound.insert("scale", #scale);
                compound.insert("seed", #seed);
                compound.insert("threshold", #threshold);
                compound.insert("high_chance", #high_chance);
                compound.insert("default_state", #default_state);
                compound.insert("low_states", #low_states);
                compound.insert("high_states", #high_states);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::DualNoise(provider) => {
            let noise = generate_feature_noise_parameters_nbt(&provider.noise);
            let scale = provider.scale;
            let seed = provider.seed;
            let slow_noise = generate_feature_noise_parameters_nbt(&provider.slow_noise);
            let slow_scale = provider.slow_scale;
            let states =
                generate_nbt_list(provider.states.iter().map(generate_block_state_data_nbt));
            let [variety_min, variety_max] = provider.variety;
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:dual_noise");
                compound.insert("noise", #noise);
                compound.insert("scale", #scale);
                compound.insert("seed", #seed);
                compound.insert("slow_noise", #slow_noise);
                compound.insert("slow_scale", #slow_scale);
                compound.insert("states", #states);
                compound.insert("variety", NbtTag::IntArray(vec![#variety_min, #variety_max]));
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::CopyProperties { source } => {
            let source = generate_block_state_provider_kind_nbt(source);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:copy_properties");
                compound.insert("source", #source);
                NbtTag::Compound(compound)
            }}
        }
        BlockStateProviderKind::RandomBlock { blocks } => {
            let blocks = generate_block_holder_set_nbt(blocks);
            quote! {{
                let mut compound = NbtCompound::new();
                compound.insert("type", "minecraft:random_block");
                compound.insert("blocks", #blocks);
                NbtTag::Compound(compound)
            }}
        }
    }
}
