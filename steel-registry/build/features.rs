//! Build-time codegen for configured and placed feature registries.

use std::fs;

use heck::ToShoutySnakeCase;
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;
use steel_utils::value_providers::{
    FloatProvider, HeightProvider, IntProvider, UniformIntProvider, VerticalAnchor,
    WeightedIntProvider,
};
use steel_utils::{Direction, Identifier, Rotation};

#[path = "feature_data.rs"]
mod feature_data;

use feature_data::*;

fn sorted_json_files(dir: &str) -> Vec<fs::DirEntry> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("{dir} missing: {err}"))
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .collect();
    files.sort_by_key(|entry| entry.file_name());
    files
}

fn resource_name(entry: &fs::DirEntry) -> String {
    entry
        .path()
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("invalid feature file name: {:?}", entry.path()))
        .to_owned()
}

fn generate_identifier(identifier: &Identifier) -> TokenStream {
    let namespace = identifier.namespace.as_ref();
    let path = identifier.path.as_ref();
    if namespace == Identifier::VANILLA_NAMESPACE {
        quote! { Identifier::vanilla_static(#path) }
    } else {
        quote! { Identifier::new_static(#namespace, #path) }
    }
}

fn vanilla_registry_ident(identifier: &Identifier, kind: &str) -> Ident {
    if identifier.namespace != Identifier::VANILLA_NAMESPACE {
        panic!("vanilla feature references non-vanilla {kind} {identifier}");
    }

    Ident::new(&identifier.path.to_shouty_snake_case(), Span::call_site())
}

fn generate_block_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "block");
    quote! { &vanilla_blocks::#ident }
}

fn generate_fluid_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "fluid");
    quote! { &vanilla_fluids::#ident }
}

fn generate_configured_feature_entry_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "configured feature");
    quote! { &crate::vanilla_configured_features::#ident }
}

fn generate_placed_feature_entry_ref(identifier: &Identifier) -> TokenStream {
    let ident = vanilla_registry_ident(identifier, "placed feature");
    quote! { &crate::vanilla_placed_features::#ident }
}

fn generate_vec<T>(values: &[T], f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let values = values.iter().map(f);
    quote! { vec![#(#values),*] }
}

fn generate_option<T>(value: &Option<T>, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    match value {
        Some(value) => {
            let value = f(value);
            quote! { Some(#value) }
        }
        None => quote! { None },
    }
}

fn generate_box<T>(value: &T, f: impl Fn(&T) -> TokenStream) -> TokenStream {
    let value = f(value);
    quote! { Box::new(#value) }
}

fn generate_offset(offset: &[i32; 3]) -> TokenStream {
    let [x, y, z] = *offset;
    quote! { [#x, #y, #z] }
}

fn generate_block_ref_list(list: &IdentifierList) -> TokenStream {
    let values = generate_vec(&list.0, generate_block_ref);
    quote! { BlockRefList(#values) }
}

fn generate_block_holder_set(set: &BlockHolderSet) -> TokenStream {
    match set {
        BlockHolderSet::Tag(tag) => {
            let tag = generate_identifier(tag);
            quote! { BlockHolderSet::Tag(#tag) }
        }
        BlockHolderSet::Entries(entries) => {
            let entries = generate_vec(entries, generate_block_ref);
            quote! { BlockHolderSet::Entries(#entries) }
        }
    }
}

fn generate_fluid_ref_list(list: &IdentifierList) -> TokenStream {
    let values = generate_vec(&list.0, generate_fluid_ref);
    quote! { FluidRefList(#values) }
}

fn generate_block_state_data(data: &BlockStateData) -> TokenStream {
    let block = generate_block_ref(&data.name);
    let properties = if data.properties.is_empty() {
        quote! { &[] }
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            let key = key.as_str();
            let value = value.as_str();
            quote! { (#key, #value) }
        });
        quote! { &[#(#entries),*] }
    };

    quote! {
        BlockStateData {
            block: #block,
            properties: #properties,
        }
    }
}

fn generate_fluid_state_data(data: &FluidStateData) -> TokenStream {
    let fluid = generate_fluid_ref(&data.name);
    let properties = if data.properties.is_empty() {
        quote! { &[] }
    } else {
        let entries = data.properties.iter().map(|(key, value)| {
            let key = key.as_str();
            let value = value.as_str();
            quote! { (#key, #value) }
        });
        quote! { &[#(#entries),*] }
    };

    quote! {
        FluidStateData {
            fluid: #fluid,
            properties: #properties,
        }
    }
}

fn generate_direction(direction: Direction) -> TokenStream {
    match direction {
        Direction::Down => quote! { Direction::Down },
        Direction::Up => quote! { Direction::Up },
        Direction::North => quote! { Direction::North },
        Direction::South => quote! { Direction::South },
        Direction::West => quote! { Direction::West },
        Direction::East => quote! { Direction::East },
    }
}

fn generate_rotation(rotation: Rotation) -> TokenStream {
    match rotation {
        Rotation::None => quote! { Rotation::None },
        Rotation::Clockwise90 => quote! { Rotation::Clockwise90 },
        Rotation::Clockwise180 => quote! { Rotation::Clockwise180 },
        Rotation::CounterClockwise90 => quote! { Rotation::CounterClockwise90 },
    }
}

fn generate_vertical_anchor(anchor: VerticalAnchor) -> TokenStream {
    match anchor {
        VerticalAnchor::Absolute(value) => quote! { VerticalAnchor::Absolute(#value) },
        VerticalAnchor::AboveBottom(value) => quote! { VerticalAnchor::AboveBottom(#value) },
        VerticalAnchor::BelowTop(value) => quote! { VerticalAnchor::BelowTop(#value) },
    }
}

fn generate_height_provider(provider: HeightProvider) -> TokenStream {
    match provider {
        HeightProvider::Constant(anchor) => {
            let anchor = generate_vertical_anchor(anchor);
            quote! { HeightProvider::Constant(#anchor) }
        }
        HeightProvider::Uniform {
            min_inclusive,
            max_inclusive,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Uniform {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        HeightProvider::Trapezoid {
            min_inclusive,
            max_inclusive,
            plateau,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::Trapezoid {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    plateau: #plateau,
                }
            }
        }
        HeightProvider::BiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::BiasedToBottom {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    inner: #inner,
                }
            }
        }
        HeightProvider::VeryBiasedToBottom {
            min_inclusive,
            max_inclusive,
            inner,
        } => {
            let min_inclusive = generate_vertical_anchor(min_inclusive);
            let max_inclusive = generate_vertical_anchor(max_inclusive);
            quote! {
                HeightProvider::VeryBiasedToBottom {
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                    inner: #inner,
                }
            }
        }
    }
}

fn generate_uniform_int_provider(provider: UniformIntProvider) -> TokenStream {
    let min_inclusive = provider.min_inclusive;
    let max_inclusive = provider.max_inclusive;
    quote! {
        UniformIntProvider {
            min_inclusive: #min_inclusive,
            max_inclusive: #max_inclusive,
        }
    }
}

fn generate_weighted_int_provider(provider: &WeightedIntProvider) -> TokenStream {
    let data = generate_int_provider(&provider.data);
    let weight = provider.weight;
    quote! { WeightedIntProvider { data: #data, weight: #weight } }
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
            let distribution = generate_vec(distribution, generate_weighted_int_provider);
            quote! { IntProvider::WeightedList { distribution: #distribution } }
        }
    }
}

fn generate_float_provider(provider: FloatProvider) -> TokenStream {
    match provider {
        FloatProvider::Constant(value) => quote! { FloatProvider::Constant(#value) },
        FloatProvider::Uniform {
            min_inclusive,
            max_exclusive,
        } => quote! {
            FloatProvider::Uniform {
                min_inclusive: #min_inclusive,
                max_exclusive: #max_exclusive,
            }
        },
        FloatProvider::Trapezoid { min, max, plateau } => quote! {
            FloatProvider::Trapezoid {
                min: #min,
                max: #max,
                plateau: #plateau,
            }
        },
        FloatProvider::ClampedNormal {
            mean,
            deviation,
            min,
            max,
        } => quote! {
            FloatProvider::ClampedNormal {
                mean: #mean,
                deviation: #deviation,
                min: #min,
                max: #max,
            }
        },
    }
}

fn generate_feature_noise_parameters(parameters: &FeatureNoiseParameters) -> TokenStream {
    let first_octave = parameters.first_octave;
    let amplitudes = parameters.amplitudes.iter();
    quote! {
        FeatureNoiseParameters {
            first_octave: #first_octave,
            amplitudes: vec![#(#amplitudes),*],
        }
    }
}

fn generate_noise_provider(provider: &NoiseProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let states = generate_vec(&provider.states, generate_block_state_data);
    quote! {
        NoiseProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            states: #states,
        }
    }
}

fn generate_noise_threshold_provider(provider: &NoiseThresholdProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let threshold = provider.threshold;
    let high_chance = provider.high_chance;
    let default_state = generate_block_state_data(&provider.default_state);
    let low_states = generate_vec(&provider.low_states, generate_block_state_data);
    let high_states = generate_vec(&provider.high_states, generate_block_state_data);
    quote! {
        NoiseThresholdProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            threshold: #threshold,
            high_chance: #high_chance,
            default_state: #default_state,
            low_states: #low_states,
            high_states: #high_states,
        }
    }
}

fn generate_dual_noise_provider(provider: &DualNoiseProvider) -> TokenStream {
    let noise = generate_feature_noise_parameters(&provider.noise);
    let scale = provider.scale;
    let seed = provider.seed;
    let slow_noise = generate_feature_noise_parameters(&provider.slow_noise);
    let slow_scale = provider.slow_scale;
    let states = generate_vec(&provider.states, generate_block_state_data);
    let [variety_min, variety_max] = provider.variety;
    quote! {
        DualNoiseProvider {
            noise: #noise,
            scale: #scale,
            seed: #seed,
            slow_noise: #slow_noise,
            slow_scale: #slow_scale,
            states: #states,
            variety: [#variety_min, #variety_max],
        }
    }
}

fn generate_weighted_block_state(entry: &WeightedBlockState) -> TokenStream {
    let data = generate_block_state_data(&entry.data);
    let weight = entry.weight;
    quote! { WeightedBlockState { data: #data, weight: #weight } }
}

fn generate_rule_based_state_provider_rule(rule: &RuleBasedStateProviderRule) -> TokenStream {
    let if_true = generate_block_predicate(&rule.if_true);
    let then = generate_block_state_provider(&rule.then);
    quote! {
        RuleBasedStateProviderRule {
            if_true: #if_true,
            then: #then,
        }
    }
}

fn generate_block_state_provider(provider: &BlockStateProvider) -> TokenStream {
    match provider {
        BlockStateProvider::Simple { state } => {
            let state = generate_block_state_data(state);
            quote! { BlockStateProvider::Simple { state: #state } }
        }
        BlockStateProvider::Weighted { entries } => {
            let entries = generate_vec(entries, generate_weighted_block_state);
            quote! { BlockStateProvider::Weighted { entries: #entries } }
        }
        BlockStateProvider::RotatedBlock { state } => {
            let state = generate_block_state_data(state);
            quote! { BlockStateProvider::RotatedBlock { state: #state } }
        }
        BlockStateProvider::RandomizedInt {
            property,
            source,
            values,
        } => {
            let property = property.as_str();
            let source = generate_box(source.as_ref(), generate_block_state_provider);
            let values = generate_int_provider(values);
            quote! {
                BlockStateProvider::RandomizedInt {
                    property: #property.to_string(),
                    source: #source,
                    values: #values,
                }
            }
        }
        BlockStateProvider::RuleBased { fallback, rules } => {
            let fallback = generate_option(fallback, |fallback| {
                generate_box(fallback.as_ref(), generate_block_state_provider)
            });
            let rules = generate_vec(rules, generate_rule_based_state_provider_rule);
            quote! {
                BlockStateProvider::RuleBased {
                    fallback: #fallback,
                    rules: #rules,
                }
            }
        }
        BlockStateProvider::Noise(provider) => {
            let provider = generate_noise_provider(provider);
            quote! { BlockStateProvider::Noise(#provider) }
        }
        BlockStateProvider::NoiseThreshold(provider) => {
            let provider = generate_noise_threshold_provider(provider);
            quote! { BlockStateProvider::NoiseThreshold(#provider) }
        }
        BlockStateProvider::DualNoise(provider) => {
            let provider = generate_dual_noise_provider(provider);
            quote! { BlockStateProvider::DualNoise(#provider) }
        }
    }
}

fn generate_block_predicate(predicate: &BlockPredicate) -> TokenStream {
    match predicate {
        BlockPredicate::True => quote! { BlockPredicate::True },
        BlockPredicate::AllOf { predicates } => {
            let predicates = generate_vec(predicates, generate_block_predicate);
            quote! { BlockPredicate::AllOf { predicates: #predicates } }
        }
        BlockPredicate::AnyOf { predicates } => {
            let predicates = generate_vec(predicates, generate_block_predicate);
            quote! { BlockPredicate::AnyOf { predicates: #predicates } }
        }
        BlockPredicate::Not { predicate } => {
            let predicate = generate_box(predicate.as_ref(), generate_block_predicate);
            quote! { BlockPredicate::Not { predicate: #predicate } }
        }
        BlockPredicate::MatchingBlockTag { tag, offset } => {
            let tag = generate_identifier(tag);
            let offset = generate_offset(offset);
            quote! { BlockPredicate::MatchingBlockTag { tag: #tag, offset: #offset } }
        }
        BlockPredicate::MatchingBlocks { blocks, offset } => {
            let blocks = generate_block_ref_list(blocks);
            let offset = generate_offset(offset);
            quote! { BlockPredicate::MatchingBlocks { blocks: #blocks, offset: #offset } }
        }
        BlockPredicate::MatchingFluids { fluids, offset } => {
            let fluids = generate_fluid_ref_list(fluids);
            let offset = generate_offset(offset);
            quote! { BlockPredicate::MatchingFluids { fluids: #fluids, offset: #offset } }
        }
        BlockPredicate::Solid { offset } => {
            let offset = generate_offset(offset);
            quote! { BlockPredicate::Solid { offset: #offset } }
        }
        BlockPredicate::WouldSurvive { state, offset } => {
            let state = generate_block_state_data(state);
            let offset = generate_offset(offset);
            quote! { BlockPredicate::WouldSurvive { state: #state, offset: #offset } }
        }
        BlockPredicate::Replaceable { offset } => {
            let offset = generate_offset(offset);
            quote! { BlockPredicate::Replaceable { offset: #offset } }
        }
        BlockPredicate::HasSturdyFace { direction, offset } => {
            let direction = generate_direction(*direction);
            let offset = generate_offset(offset);
            quote! { BlockPredicate::HasSturdyFace { direction: #direction, offset: #offset } }
        }
        BlockPredicate::InsideWorldBounds { offset } => {
            let offset = generate_offset(offset);
            quote! { BlockPredicate::InsideWorldBounds { offset: #offset } }
        }
    }
}

fn generate_configured_feature_ref(feature: &ConfiguredFeatureRef) -> TokenStream {
    match feature {
        ConfiguredFeatureRef::Reference(identifier) => {
            let reference = generate_configured_feature_entry_ref(identifier);
            quote! { ConfiguredFeatureRef::Reference(#reference) }
        }
        ConfiguredFeatureRef::Inline(kind) => {
            let kind = generate_box(kind.as_ref(), generate_configured_feature_kind);
            quote! { ConfiguredFeatureRef::Inline(#kind) }
        }
    }
}

fn generate_placed_feature_ref(feature: &PlacedFeatureRef) -> TokenStream {
    match feature {
        PlacedFeatureRef::Reference(identifier) => {
            let reference = generate_placed_feature_entry_ref(identifier);
            quote! { PlacedFeatureRef::Reference(#reference) }
        }
        PlacedFeatureRef::Inline(data) => {
            let data = generate_box(data.as_ref(), generate_placed_feature_data);
            quote! { PlacedFeatureRef::Inline(#data) }
        }
    }
}

fn generate_placed_feature_data(data: &PlacedFeatureData) -> TokenStream {
    let feature = generate_configured_feature_ref(&data.feature);
    let placement = generate_vec(&data.placement, generate_placement_modifier);
    quote! {
        PlacedFeatureData {
            feature: #feature,
            placement: #placement,
        }
    }
}

fn generate_feature_heightmap(heightmap: FeatureHeightmap) -> TokenStream {
    match heightmap {
        FeatureHeightmap::WorldSurface => quote! { FeatureHeightmap::WorldSurface },
        FeatureHeightmap::MotionBlocking => quote! { FeatureHeightmap::MotionBlocking },
        FeatureHeightmap::MotionBlockingNoLeaves => {
            quote! { FeatureHeightmap::MotionBlockingNoLeaves }
        }
        FeatureHeightmap::OceanFloor => quote! { FeatureHeightmap::OceanFloor },
        FeatureHeightmap::WorldSurfaceWg => quote! { FeatureHeightmap::WorldSurfaceWg },
        FeatureHeightmap::OceanFloorWg => quote! { FeatureHeightmap::OceanFloorWg },
    }
}

fn generate_placement_modifier(modifier: &PlacementModifier) -> TokenStream {
    match modifier {
        PlacementModifier::Biome => quote! { PlacementModifier::Biome },
        PlacementModifier::BlockPredicateFilter { predicate } => {
            let predicate = generate_block_predicate(predicate);
            quote! { PlacementModifier::BlockPredicateFilter { predicate: #predicate } }
        }
        PlacementModifier::Count { count } => {
            let count = generate_int_provider(count);
            quote! { PlacementModifier::Count { count: #count } }
        }
        PlacementModifier::CountOnEveryLayer { count } => {
            let count = generate_int_provider(count);
            quote! { PlacementModifier::CountOnEveryLayer { count: #count } }
        }
        PlacementModifier::EnvironmentScan {
            direction_of_search,
            target_condition,
            allowed_search_condition,
            max_steps,
        } => {
            let direction_of_search = generate_direction(*direction_of_search);
            let target_condition = generate_block_predicate(target_condition);
            let allowed_search_condition =
                generate_option(allowed_search_condition, generate_block_predicate);
            quote! {
                PlacementModifier::EnvironmentScan {
                    direction_of_search: #direction_of_search,
                    target_condition: #target_condition,
                    allowed_search_condition: #allowed_search_condition,
                    max_steps: #max_steps,
                }
            }
        }
        PlacementModifier::FixedPlacement { positions } => {
            let positions = generate_vec(positions, generate_offset);
            quote! { PlacementModifier::FixedPlacement { positions: #positions } }
        }
        PlacementModifier::HeightRange { height } => {
            let height = generate_height_provider(*height);
            quote! { PlacementModifier::HeightRange { height: #height } }
        }
        PlacementModifier::Heightmap { heightmap } => {
            let heightmap = generate_feature_heightmap(*heightmap);
            quote! { PlacementModifier::Heightmap { heightmap: #heightmap } }
        }
        PlacementModifier::InSquare => quote! { PlacementModifier::InSquare },
        PlacementModifier::NoiseBasedCount {
            noise_to_count_ratio,
            noise_factor,
            noise_offset,
        } => quote! {
            PlacementModifier::NoiseBasedCount {
                noise_to_count_ratio: #noise_to_count_ratio,
                noise_factor: #noise_factor,
                noise_offset: #noise_offset,
            }
        },
        PlacementModifier::NoiseThresholdCount {
            noise_level,
            below_noise,
            above_noise,
        } => quote! {
            PlacementModifier::NoiseThresholdCount {
                noise_level: #noise_level,
                below_noise: #below_noise,
                above_noise: #above_noise,
            }
        },
        PlacementModifier::RandomOffset {
            xz_spread,
            y_spread,
        } => {
            let xz_spread = generate_int_provider(xz_spread);
            let y_spread = generate_int_provider(y_spread);
            quote! {
                PlacementModifier::RandomOffset {
                    xz_spread: #xz_spread,
                    y_spread: #y_spread,
                }
            }
        }
        PlacementModifier::RarityFilter { chance } => {
            quote! { PlacementModifier::RarityFilter { chance: #chance } }
        }
        PlacementModifier::SurfaceRelativeThresholdFilter {
            heightmap,
            min_inclusive,
            max_inclusive,
        } => {
            let heightmap = generate_feature_heightmap(*heightmap);
            let min_inclusive = generate_option(min_inclusive, |value| quote! { #value });
            let max_inclusive = generate_option(max_inclusive, |value| quote! { #value });
            quote! {
                PlacementModifier::SurfaceRelativeThresholdFilter {
                    heightmap: #heightmap,
                    min_inclusive: #min_inclusive,
                    max_inclusive: #max_inclusive,
                }
            }
        }
        PlacementModifier::SurfaceWaterDepthFilter { max_water_depth } => {
            quote! {
                PlacementModifier::SurfaceWaterDepthFilter {
                    max_water_depth: #max_water_depth,
                }
            }
        }
    }
}

fn generate_block_column_layer(layer: &BlockColumnLayer) -> TokenStream {
    let height = generate_int_provider(&layer.height);
    let provider = generate_block_state_provider(&layer.provider);
    quote! { BlockColumnLayer { height: #height, provider: #provider } }
}

fn generate_end_spike(spike: &EndSpike) -> TokenStream {
    let center_x = spike.center_x;
    let center_z = spike.center_z;
    let radius = spike.radius;
    let height = spike.height;
    let guarded = spike.guarded;
    quote! {
        EndSpike {
            center_x: #center_x,
            center_z: #center_z,
            radius: #radius,
            height: #height,
            guarded: #guarded,
        }
    }
}

fn generate_geode_block_settings(settings: &GeodeBlockSettings) -> TokenStream {
    let filling_provider = generate_block_state_provider(&settings.filling_provider);
    let inner_layer_provider = generate_block_state_provider(&settings.inner_layer_provider);
    let alternate_inner_layer_provider =
        generate_block_state_provider(&settings.alternate_inner_layer_provider);
    let middle_layer_provider = generate_block_state_provider(&settings.middle_layer_provider);
    let outer_layer_provider = generate_block_state_provider(&settings.outer_layer_provider);
    let inner_placements = generate_vec(&settings.inner_placements, generate_block_state_data);
    let cannot_replace = generate_identifier(&settings.cannot_replace);
    let invalid_blocks = generate_identifier(&settings.invalid_blocks);
    quote! {
        GeodeBlockSettings {
            filling_provider: #filling_provider,
            inner_layer_provider: #inner_layer_provider,
            alternate_inner_layer_provider: #alternate_inner_layer_provider,
            middle_layer_provider: #middle_layer_provider,
            outer_layer_provider: #outer_layer_provider,
            inner_placements: #inner_placements,
            cannot_replace: #cannot_replace,
            invalid_blocks: #invalid_blocks,
        }
    }
}

fn generate_geode_layer_settings(settings: &GeodeLayerSettings) -> TokenStream {
    let filling = settings.filling;
    let inner_layer = settings.inner_layer;
    let middle_layer = settings.middle_layer;
    let outer_layer = settings.outer_layer;
    quote! {
        GeodeLayerSettings {
            filling: #filling,
            inner_layer: #inner_layer,
            middle_layer: #middle_layer,
            outer_layer: #outer_layer,
        }
    }
}

fn generate_geode_crack_settings(settings: &GeodeCrackSettings) -> TokenStream {
    let generate_crack_chance = settings.generate_crack_chance;
    let base_crack_size = settings.base_crack_size;
    let crack_point_offset = settings.crack_point_offset;
    quote! {
        GeodeCrackSettings {
            generate_crack_chance: #generate_crack_chance,
            base_crack_size: #base_crack_size,
            crack_point_offset: #crack_point_offset,
        }
    }
}

fn generate_ore_target(target: &OreTarget) -> TokenStream {
    let target_rule = generate_rule_test(&target.target);
    let state = generate_block_state_data(&target.state);
    quote! { OreTarget { target: #target_rule, state: #state } }
}

fn generate_rule_test(rule: &RuleTest) -> TokenStream {
    match rule {
        RuleTest::BlockMatch { block } => {
            let block = generate_block_ref(block);
            quote! { RuleTest::BlockMatch { block: #block } }
        }
        RuleTest::TagMatch { tag } => {
            let tag = generate_identifier(tag);
            quote! { RuleTest::TagMatch { tag: #tag } }
        }
    }
}

fn generate_weighted_placed_feature(feature: &WeightedPlacedFeature) -> TokenStream {
    let chance = feature.chance;
    let feature = generate_placed_feature_ref(&feature.feature);
    quote! { WeightedPlacedFeature { chance: #chance, feature: #feature } }
}

fn generate_weighted_random_placed_feature(feature: &WeightedRandomPlacedFeature) -> TokenStream {
    let data = generate_placed_feature_ref(&feature.data);
    let weight = feature.weight;
    quote! { WeightedRandomPlacedFeature { data: #data, weight: #weight } }
}

fn generate_template_entry(entry: &TemplateEntry) -> TokenStream {
    let id = generate_identifier(&entry.id);
    let rotations = generate_vec(&entry.rotations, |rotation| generate_rotation(*rotation));
    quote! { TemplateEntry { id: #id, rotations: #rotations } }
}

fn generate_weighted_template_entry(entry: &WeightedTemplateEntry) -> TokenStream {
    let data = generate_template_entry(&entry.data);
    let weight = entry.weight;
    quote! { WeightedTemplateEntry { data: #data, weight: #weight } }
}

fn generate_trunk_placer_base(base: &TrunkPlacerBase) -> TokenStream {
    let base_height = base.base_height;
    let height_rand_a = base.height_rand_a;
    let height_rand_b = base.height_rand_b;
    quote! {
        TrunkPlacerBase {
            base_height: #base_height,
            height_rand_a: #height_rand_a,
            height_rand_b: #height_rand_b,
        }
    }
}

fn generate_trunk_placer(placer: &TrunkPlacer) -> TokenStream {
    match placer {
        TrunkPlacer::Straight(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Straight(#base) }
        }
        TrunkPlacer::Giant(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Giant(#base) }
        }
        TrunkPlacer::Fancy(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Fancy(#base) }
        }
        TrunkPlacer::Forking(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::Forking(#base) }
        }
        TrunkPlacer::DarkOak(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::DarkOak(#base) }
        }
        TrunkPlacer::MegaJungle(base) => {
            let base = generate_trunk_placer_base(base);
            quote! { TrunkPlacer::MegaJungle(#base) }
        }
        TrunkPlacer::Bending(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let min_height_for_leaves = placer.min_height_for_leaves;
            let bend_length = generate_int_provider(&placer.bend_length);
            quote! {
                TrunkPlacer::Bending(BendingTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    min_height_for_leaves: #min_height_for_leaves,
                    bend_length: #bend_length,
                })
            }
        }
        TrunkPlacer::UpwardsBranching(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let extra_branch_steps = generate_int_provider(&placer.extra_branch_steps);
            let extra_branch_length = generate_int_provider(&placer.extra_branch_length);
            let place_branch_per_log_probability = placer.place_branch_per_log_probability;
            let can_grow_through = generate_identifier(&placer.can_grow_through);
            quote! {
                TrunkPlacer::UpwardsBranching(UpwardsBranchingTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    extra_branch_steps: #extra_branch_steps,
                    extra_branch_length: #extra_branch_length,
                    place_branch_per_log_probability: #place_branch_per_log_probability,
                    can_grow_through: #can_grow_through,
                })
            }
        }
        TrunkPlacer::Cherry(placer) => {
            let base_height = placer.base_height;
            let height_rand_a = placer.height_rand_a;
            let height_rand_b = placer.height_rand_b;
            let branch_count = generate_int_provider(&placer.branch_count);
            let branch_horizontal_length = generate_int_provider(&placer.branch_horizontal_length);
            let branch_start_offset_from_top =
                generate_uniform_int_provider(placer.branch_start_offset_from_top);
            let branch_end_offset_from_top =
                generate_int_provider(&placer.branch_end_offset_from_top);
            quote! {
                TrunkPlacer::Cherry(CherryTrunkPlacer {
                    base_height: #base_height,
                    height_rand_a: #height_rand_a,
                    height_rand_b: #height_rand_b,
                    branch_count: #branch_count,
                    branch_horizontal_length: #branch_horizontal_length,
                    branch_start_offset_from_top: #branch_start_offset_from_top,
                    branch_end_offset_from_top: #branch_end_offset_from_top,
                })
            }
        }
    }
}

fn generate_blob_foliage_placer(placer: &BlobFoliagePlacer) -> TokenStream {
    let radius = generate_int_provider(&placer.radius);
    let offset = generate_int_provider(&placer.offset);
    let height = generate_int_provider(&placer.height);
    quote! { BlobFoliagePlacer { radius: #radius, offset: #offset, height: #height } }
}

fn generate_foliage_placer_base(placer: &FoliagePlacerBase) -> TokenStream {
    let radius = generate_int_provider(&placer.radius);
    let offset = generate_int_provider(&placer.offset);
    quote! { FoliagePlacerBase { radius: #radius, offset: #offset } }
}

fn generate_foliage_placer(placer: &FoliagePlacer) -> TokenStream {
    match placer {
        FoliagePlacer::Blob(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Blob(#placer) }
        }
        FoliagePlacer::Spruce(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let trunk_height = generate_int_provider(&placer.trunk_height);
            quote! {
                FoliagePlacer::Spruce(SpruceFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    trunk_height: #trunk_height,
                })
            }
        }
        FoliagePlacer::Pine(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let height = generate_int_provider(&placer.height);
            quote! {
                FoliagePlacer::Pine(PineFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    height: #height,
                })
            }
        }
        FoliagePlacer::Acacia(placer) => {
            let placer = generate_foliage_placer_base(placer);
            quote! { FoliagePlacer::Acacia(#placer) }
        }
        FoliagePlacer::Bush(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Bush(#placer) }
        }
        FoliagePlacer::Fancy(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Fancy(#placer) }
        }
        FoliagePlacer::Jungle(placer) => {
            let placer = generate_blob_foliage_placer(placer);
            quote! { FoliagePlacer::Jungle(#placer) }
        }
        FoliagePlacer::MegaPine(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let crown_height = generate_int_provider(&placer.crown_height);
            quote! {
                FoliagePlacer::MegaPine(MegaPineFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    crown_height: #crown_height,
                })
            }
        }
        FoliagePlacer::DarkOak(placer) => {
            let placer = generate_foliage_placer_base(placer);
            quote! { FoliagePlacer::DarkOak(#placer) }
        }
        FoliagePlacer::RandomSpread(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let foliage_height = placer.foliage_height;
            let leaf_placement_attempts = placer.leaf_placement_attempts;
            quote! {
                FoliagePlacer::RandomSpread(RandomSpreadFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    foliage_height: #foliage_height,
                    leaf_placement_attempts: #leaf_placement_attempts,
                })
            }
        }
        FoliagePlacer::Cherry(placer) => {
            let radius = generate_int_provider(&placer.radius);
            let offset = generate_int_provider(&placer.offset);
            let height = generate_int_provider(&placer.height);
            let wide_bottom_layer_hole_chance = placer.wide_bottom_layer_hole_chance;
            let corner_hole_chance = placer.corner_hole_chance;
            let hanging_leaves_chance = placer.hanging_leaves_chance;
            let hanging_leaves_extension_chance = placer.hanging_leaves_extension_chance;
            quote! {
                FoliagePlacer::Cherry(CherryFoliagePlacer {
                    radius: #radius,
                    offset: #offset,
                    height: #height,
                    wide_bottom_layer_hole_chance: #wide_bottom_layer_hole_chance,
                    corner_hole_chance: #corner_hole_chance,
                    hanging_leaves_chance: #hanging_leaves_chance,
                    hanging_leaves_extension_chance: #hanging_leaves_extension_chance,
                })
            }
        }
    }
}

fn generate_feature_size(size: &FeatureSize) -> TokenStream {
    match size {
        FeatureSize::TwoLayers(size) => {
            let limit = size.limit;
            let lower_size = size.lower_size;
            let upper_size = size.upper_size;
            let min_clipped_height =
                generate_option(&size.min_clipped_height, |value| quote! { #value });
            quote! {
                FeatureSize::TwoLayers(TwoLayersFeatureSize {
                    limit: #limit,
                    lower_size: #lower_size,
                    upper_size: #upper_size,
                    min_clipped_height: #min_clipped_height,
                })
            }
        }
        FeatureSize::ThreeLayers(size) => {
            let limit = size.limit;
            let lower_size = size.lower_size;
            let middle_size = size.middle_size;
            let upper_limit = size.upper_limit;
            let upper_size = size.upper_size;
            let min_clipped_height =
                generate_option(&size.min_clipped_height, |value| quote! { #value });
            quote! {
                FeatureSize::ThreeLayers(ThreeLayersFeatureSize {
                    limit: #limit,
                    lower_size: #lower_size,
                    middle_size: #middle_size,
                    upper_limit: #upper_limit,
                    upper_size: #upper_size,
                    min_clipped_height: #min_clipped_height,
                })
            }
        }
    }
}

fn generate_above_root_placement(placement: &AboveRootPlacement) -> TokenStream {
    let above_root_provider = generate_block_state_provider(&placement.above_root_provider);
    let above_root_placement_chance = placement.above_root_placement_chance;
    quote! {
        AboveRootPlacement {
            above_root_provider: #above_root_provider,
            above_root_placement_chance: #above_root_placement_chance,
        }
    }
}

fn generate_mangrove_root_placement(placement: &MangroveRootPlacement) -> TokenStream {
    let can_grow_through = generate_identifier(&placement.can_grow_through);
    let muddy_roots_in = generate_vec(&placement.muddy_roots_in, generate_identifier);
    let muddy_roots_provider = generate_block_state_provider(&placement.muddy_roots_provider);
    let max_root_width = placement.max_root_width;
    let max_root_length = placement.max_root_length;
    let random_skew_chance = placement.random_skew_chance;
    quote! {
        MangroveRootPlacement {
            can_grow_through: #can_grow_through,
            muddy_roots_in: #muddy_roots_in,
            muddy_roots_provider: #muddy_roots_provider,
            max_root_width: #max_root_width,
            max_root_length: #max_root_length,
            random_skew_chance: #random_skew_chance,
        }
    }
}

fn generate_root_placer(placer: &RootPlacer) -> TokenStream {
    match placer {
        RootPlacer::Mangrove(placer) => {
            let trunk_offset_y = generate_int_provider(&placer.trunk_offset_y);
            let root_provider = generate_block_state_provider(&placer.root_provider);
            let above_root_placement = generate_above_root_placement(&placer.above_root_placement);
            let mangrove_root_placement =
                generate_mangrove_root_placement(&placer.mangrove_root_placement);
            quote! {
                RootPlacer::Mangrove(MangroveRootPlacer {
                    trunk_offset_y: #trunk_offset_y,
                    root_provider: #root_provider,
                    above_root_placement: #above_root_placement,
                    mangrove_root_placement: #mangrove_root_placement,
                })
            }
        }
    }
}

fn generate_tree_decorator(decorator: &TreeDecorator) -> TokenStream {
    match decorator {
        TreeDecorator::AlterGround { provider } => {
            let provider = generate_block_state_provider(provider);
            quote! { TreeDecorator::AlterGround { provider: #provider } }
        }
        TreeDecorator::Beehive { probability } => {
            quote! { TreeDecorator::Beehive { probability: #probability } }
        }
        TreeDecorator::Cocoa { probability } => {
            quote! { TreeDecorator::Cocoa { probability: #probability } }
        }
        TreeDecorator::CreakingHeart { probability } => {
            quote! { TreeDecorator::CreakingHeart { probability: #probability } }
        }
        TreeDecorator::LeaveVine { probability } => {
            quote! { TreeDecorator::LeaveVine { probability: #probability } }
        }
        TreeDecorator::TrunkVine => quote! { TreeDecorator::TrunkVine },
        TreeDecorator::AttachedToLeaves(decorator) => {
            let probability = decorator.probability;
            let exclusion_radius_xz = decorator.exclusion_radius_xz;
            let exclusion_radius_y = decorator.exclusion_radius_y;
            let required_empty_blocks = decorator.required_empty_blocks;
            let block_provider = generate_block_state_provider(&decorator.block_provider);
            let directions = generate_vec(&decorator.directions, |direction| {
                generate_direction(*direction)
            });
            quote! {
                TreeDecorator::AttachedToLeaves(AttachedToLeavesDecorator {
                    probability: #probability,
                    exclusion_radius_xz: #exclusion_radius_xz,
                    exclusion_radius_y: #exclusion_radius_y,
                    required_empty_blocks: #required_empty_blocks,
                    block_provider: #block_provider,
                    directions: #directions,
                })
            }
        }
        TreeDecorator::AttachedToLogs(decorator) => {
            let probability = decorator.probability;
            let block_provider = generate_block_state_provider(&decorator.block_provider);
            let directions = generate_vec(&decorator.directions, |direction| {
                generate_direction(*direction)
            });
            quote! {
                TreeDecorator::AttachedToLogs(AttachedToLogsDecorator {
                    probability: #probability,
                    block_provider: #block_provider,
                    directions: #directions,
                })
            }
        }
        TreeDecorator::PlaceOnGround(decorator) => {
            let block_state_provider =
                generate_block_state_provider(&decorator.block_state_provider);
            let tries = decorator.tries;
            let radius = decorator.radius;
            let height = decorator.height;
            quote! {
                TreeDecorator::PlaceOnGround(PlaceOnGroundDecorator {
                    block_state_provider: #block_state_provider,
                    tries: #tries,
                    radius: #radius,
                    height: #height,
                })
            }
        }
        TreeDecorator::PaleMoss {
            leaves_probability,
            trunk_probability,
            ground_probability,
        } => quote! {
            TreeDecorator::PaleMoss {
                leaves_probability: #leaves_probability,
                trunk_probability: #trunk_probability,
                ground_probability: #ground_probability,
            }
        },
    }
}

fn generate_vertical_surface(surface: VerticalSurface) -> TokenStream {
    match surface {
        VerticalSurface::Floor => quote! { VerticalSurface::Floor },
        VerticalSurface::Ceiling => quote! { VerticalSurface::Ceiling },
    }
}

fn generate_huge_mushroom_kind(
    variant_name: &str,
    config: &HugeMushroomConfiguration,
) -> TokenStream {
    let variant = Ident::new(variant_name, Span::call_site());
    let cap_provider = generate_block_state_provider(&config.cap_provider);
    let stem_provider = generate_block_state_provider(&config.stem_provider);
    let foliage_radius = config.foliage_radius;
    let can_place_on = generate_block_predicate(&config.can_place_on);
    quote! {
        ConfiguredFeatureKind::#variant(HugeMushroomConfiguration {
            cap_provider: #cap_provider,
            stem_provider: #stem_provider,
            foliage_radius: #foliage_radius,
            can_place_on: #can_place_on,
        })
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "keeps feature registry codegen aligned with the runtime feature enum"
)]
fn generate_configured_feature_kind(kind: &ConfiguredFeatureKind) -> TokenStream {
    match kind {
        ConfiguredFeatureKind::Bamboo(config) => {
            let probability = config.probability;
            quote! {
                ConfiguredFeatureKind::Bamboo(BambooConfiguration {
                    probability: #probability,
                })
            }
        }
        ConfiguredFeatureKind::BasaltColumns(config) => {
            let height = generate_int_provider(&config.height);
            let reach = generate_int_provider(&config.reach);
            quote! {
                ConfiguredFeatureKind::BasaltColumns(BasaltColumnsConfiguration {
                    height: #height,
                    reach: #reach,
                })
            }
        }
        ConfiguredFeatureKind::BasaltPillar => quote! { ConfiguredFeatureKind::BasaltPillar },
        ConfiguredFeatureKind::BlockBlob(config) => {
            let state = generate_block_state_data(&config.state);
            let can_place_on = generate_block_predicate(&config.can_place_on);
            quote! {
                ConfiguredFeatureKind::BlockBlob(BlockBlobConfiguration {
                    state: #state,
                    can_place_on: #can_place_on,
                })
            }
        }
        ConfiguredFeatureKind::BlockColumn(config) => {
            let direction = generate_direction(config.direction);
            let allowed_placement = generate_block_predicate(&config.allowed_placement);
            let layers = generate_vec(&config.layers, generate_block_column_layer);
            let prioritize_tip = config.prioritize_tip;
            quote! {
                ConfiguredFeatureKind::BlockColumn(BlockColumnConfiguration {
                    direction: #direction,
                    allowed_placement: #allowed_placement,
                    layers: #layers,
                    prioritize_tip: #prioritize_tip,
                })
            }
        }
        ConfiguredFeatureKind::BlockPile(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            quote! {
                ConfiguredFeatureKind::BlockPile(BlockPileConfiguration {
                    state_provider: #state_provider,
                })
            }
        }
        ConfiguredFeatureKind::BlueIce => quote! { ConfiguredFeatureKind::BlueIce },
        ConfiguredFeatureKind::BonusChest => quote! { ConfiguredFeatureKind::BonusChest },
        ConfiguredFeatureKind::ChorusPlant => quote! { ConfiguredFeatureKind::ChorusPlant },
        ConfiguredFeatureKind::CoralClaw => quote! { ConfiguredFeatureKind::CoralClaw },
        ConfiguredFeatureKind::CoralMushroom => quote! { ConfiguredFeatureKind::CoralMushroom },
        ConfiguredFeatureKind::CoralTree => quote! { ConfiguredFeatureKind::CoralTree },
        ConfiguredFeatureKind::DeltaFeature(config) => {
            let contents = generate_block_state_data(&config.contents);
            let rim = generate_block_state_data(&config.rim);
            let size = generate_int_provider(&config.size);
            let rim_size = generate_int_provider(&config.rim_size);
            quote! {
                ConfiguredFeatureKind::DeltaFeature(DeltaFeatureConfiguration {
                    contents: #contents,
                    rim: #rim,
                    size: #size,
                    rim_size: #rim_size,
                })
            }
        }
        ConfiguredFeatureKind::DesertWell => quote! { ConfiguredFeatureKind::DesertWell },
        ConfiguredFeatureKind::Disk(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            let target = generate_block_predicate(&config.target);
            let radius = generate_int_provider(&config.radius);
            let half_height = config.half_height;
            quote! {
                ConfiguredFeatureKind::Disk(DiskConfiguration {
                    state_provider: #state_provider,
                    target: #target,
                    radius: #radius,
                    half_height: #half_height,
                })
            }
        }
        ConfiguredFeatureKind::DripstoneCluster(config) => {
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let height = generate_int_provider(&config.height);
            let radius = generate_int_provider(&config.radius);
            let max_stalagmite_stalactite_height_diff =
                config.max_stalagmite_stalactite_height_diff;
            let height_deviation = config.height_deviation;
            let dripstone_block_layer_thickness =
                generate_int_provider(&config.dripstone_block_layer_thickness);
            let density = generate_float_provider(config.density);
            let wetness = generate_float_provider(config.wetness);
            let chance_of_dripstone_column_at_max_distance_from_center =
                config.chance_of_dripstone_column_at_max_distance_from_center;
            let max_distance_from_center_affecting_height_bias =
                config.max_distance_from_center_affecting_height_bias;
            let max_distance_from_edge_affecting_chance_of_dripstone_column =
                config.max_distance_from_edge_affecting_chance_of_dripstone_column;
            quote! {
                ConfiguredFeatureKind::DripstoneCluster(DripstoneClusterConfiguration {
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    height: #height,
                    radius: #radius,
                    max_stalagmite_stalactite_height_diff: #max_stalagmite_stalactite_height_diff,
                    height_deviation: #height_deviation,
                    dripstone_block_layer_thickness: #dripstone_block_layer_thickness,
                    density: #density,
                    wetness: #wetness,
                    chance_of_dripstone_column_at_max_distance_from_center: #chance_of_dripstone_column_at_max_distance_from_center,
                    max_distance_from_center_affecting_height_bias: #max_distance_from_center_affecting_height_bias,
                    max_distance_from_edge_affecting_chance_of_dripstone_column: #max_distance_from_edge_affecting_chance_of_dripstone_column,
                })
            }
        }
        ConfiguredFeatureKind::SpeleothemCluster(config) => {
            let base_block = generate_block_state_data(&config.base_block);
            let pointed_block = generate_block_state_data(&config.pointed_block);
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let height = generate_int_provider(&config.height);
            let radius = generate_int_provider(&config.radius);
            let max_stalagmite_stalactite_height_diff =
                config.max_stalagmite_stalactite_height_diff;
            let height_deviation = config.height_deviation;
            let speleothem_block_layer_thickness =
                generate_int_provider(&config.speleothem_block_layer_thickness);
            let density = generate_float_provider(config.density);
            let wetness = generate_float_provider(config.wetness);
            let chance_of_speleothem_at_max_distance_from_center =
                config.chance_of_speleothem_at_max_distance_from_center;
            let max_distance_from_edge_affecting_chance_of_speleothem =
                config.max_distance_from_edge_affecting_chance_of_speleothem;
            let max_distance_from_center_affecting_height_bias =
                config.max_distance_from_center_affecting_height_bias;
            quote! {
                ConfiguredFeatureKind::SpeleothemCluster(SpeleothemClusterConfiguration {
                    base_block: #base_block,
                    pointed_block: #pointed_block,
                    replaceable_blocks: #replaceable_blocks,
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    height: #height,
                    radius: #radius,
                    max_stalagmite_stalactite_height_diff: #max_stalagmite_stalactite_height_diff,
                    height_deviation: #height_deviation,
                    speleothem_block_layer_thickness: #speleothem_block_layer_thickness,
                    density: #density,
                    wetness: #wetness,
                    chance_of_speleothem_at_max_distance_from_center: #chance_of_speleothem_at_max_distance_from_center,
                    max_distance_from_edge_affecting_chance_of_speleothem: #max_distance_from_edge_affecting_chance_of_speleothem,
                    max_distance_from_center_affecting_height_bias: #max_distance_from_center_affecting_height_bias,
                })
            }
        }
        ConfiguredFeatureKind::EndGateway(config) => {
            let exit = generate_option(&config.exit, generate_offset);
            let exact = config.exact;
            quote! {
                ConfiguredFeatureKind::EndGateway(EndGatewayConfiguration {
                    exit: #exit,
                    exact: #exact,
                })
            }
        }
        ConfiguredFeatureKind::EndIsland => quote! { ConfiguredFeatureKind::EndIsland },
        ConfiguredFeatureKind::EndPlatform => quote! { ConfiguredFeatureKind::EndPlatform },
        ConfiguredFeatureKind::EndSpike(config) => {
            let spikes = generate_vec(&config.spikes, generate_end_spike);
            let crystal_invulnerable = config.crystal_invulnerable;
            let crystal_beam_target = generate_option(&config.crystal_beam_target, generate_offset);
            quote! {
                ConfiguredFeatureKind::EndSpike(EndSpikeConfiguration {
                    spikes: #spikes,
                    crystal_invulnerable: #crystal_invulnerable,
                    crystal_beam_target: #crystal_beam_target,
                })
            }
        }
        ConfiguredFeatureKind::FallenTree(config) => {
            let trunk_provider = generate_block_state_provider(&config.trunk_provider);
            let log_length = generate_int_provider(&config.log_length);
            let stump_decorators = generate_vec(&config.stump_decorators, generate_tree_decorator);
            let log_decorators = generate_vec(&config.log_decorators, generate_tree_decorator);
            quote! {
                ConfiguredFeatureKind::FallenTree(FallenTreeConfiguration {
                    trunk_provider: #trunk_provider,
                    log_length: #log_length,
                    stump_decorators: #stump_decorators,
                    log_decorators: #log_decorators,
                })
            }
        }
        ConfiguredFeatureKind::Fossil(config) => {
            let fossil_structures = generate_vec(&config.fossil_structures, generate_identifier);
            let overlay_structures = generate_vec(&config.overlay_structures, generate_identifier);
            let fossil_processors = generate_identifier(&config.fossil_processors);
            let overlay_processors = generate_identifier(&config.overlay_processors);
            let max_empty_corners_allowed = config.max_empty_corners_allowed;
            quote! {
                ConfiguredFeatureKind::Fossil(FossilConfiguration {
                    fossil_structures: #fossil_structures,
                    overlay_structures: #overlay_structures,
                    fossil_processors: #fossil_processors,
                    overlay_processors: #overlay_processors,
                    max_empty_corners_allowed: #max_empty_corners_allowed,
                })
            }
        }
        ConfiguredFeatureKind::FreezeTopLayer => quote! { ConfiguredFeatureKind::FreezeTopLayer },
        ConfiguredFeatureKind::Geode(config) => {
            let blocks = generate_geode_block_settings(&config.blocks);
            let layers = generate_geode_layer_settings(&config.layers);
            let crack = generate_geode_crack_settings(&config.crack);
            let use_potential_placements_chance = config.use_potential_placements_chance;
            let use_alternate_layer0_chance = config.use_alternate_layer0_chance;
            let placements_require_layer0_alternate = config.placements_require_layer0_alternate;
            let outer_wall_distance = generate_int_provider(&config.outer_wall_distance);
            let distribution_points = generate_int_provider(&config.distribution_points);
            let point_offset = generate_int_provider(&config.point_offset);
            let min_gen_offset = config.min_gen_offset;
            let max_gen_offset = config.max_gen_offset;
            let invalid_blocks_threshold = config.invalid_blocks_threshold;
            let noise_multiplier = config.noise_multiplier;
            quote! {
                ConfiguredFeatureKind::Geode(GeodeConfiguration {
                    blocks: #blocks,
                    layers: #layers,
                    crack: #crack,
                    use_potential_placements_chance: #use_potential_placements_chance,
                    use_alternate_layer0_chance: #use_alternate_layer0_chance,
                    placements_require_layer0_alternate: #placements_require_layer0_alternate,
                    outer_wall_distance: #outer_wall_distance,
                    distribution_points: #distribution_points,
                    point_offset: #point_offset,
                    min_gen_offset: #min_gen_offset,
                    max_gen_offset: #max_gen_offset,
                    invalid_blocks_threshold: #invalid_blocks_threshold,
                    noise_multiplier: #noise_multiplier,
                })
            }
        }
        ConfiguredFeatureKind::GlowstoneBlob => quote! { ConfiguredFeatureKind::GlowstoneBlob },
        ConfiguredFeatureKind::HugeBrownMushroom(config) => {
            generate_huge_mushroom_kind("HugeBrownMushroom", config)
        }
        ConfiguredFeatureKind::HugeFungus(config) => {
            let valid_base_block = generate_block_state_data(&config.valid_base_block);
            let stem_state = generate_block_state_data(&config.stem_state);
            let hat_state = generate_block_state_data(&config.hat_state);
            let decor_state = generate_block_state_data(&config.decor_state);
            let replaceable_blocks = generate_block_predicate(&config.replaceable_blocks);
            let planted = config.planted;
            quote! {
                ConfiguredFeatureKind::HugeFungus(HugeFungusConfiguration {
                    valid_base_block: #valid_base_block,
                    stem_state: #stem_state,
                    hat_state: #hat_state,
                    decor_state: #decor_state,
                    replaceable_blocks: #replaceable_blocks,
                    planted: #planted,
                })
            }
        }
        ConfiguredFeatureKind::HugeRedMushroom(config) => {
            generate_huge_mushroom_kind("HugeRedMushroom", config)
        }
        ConfiguredFeatureKind::Iceberg(state) => {
            let state = generate_block_state_data(state);
            quote! { ConfiguredFeatureKind::Iceberg(#state) }
        }
        ConfiguredFeatureKind::Kelp => quote! { ConfiguredFeatureKind::Kelp },
        ConfiguredFeatureKind::Lake(config) => {
            let fluid = generate_block_state_provider(&config.fluid);
            let barrier = generate_block_state_provider(&config.barrier);
            let can_place_feature = generate_block_predicate(&config.can_place_feature);
            let can_replace_with_air_or_fluid =
                generate_block_predicate(&config.can_replace_with_air_or_fluid);
            let can_replace_with_barrier =
                generate_block_predicate(&config.can_replace_with_barrier);
            quote! {
                ConfiguredFeatureKind::Lake(LakeConfiguration {
                    fluid: #fluid,
                    barrier: #barrier,
                    can_place_feature: #can_place_feature,
                    can_replace_with_air_or_fluid: #can_replace_with_air_or_fluid,
                    can_replace_with_barrier: #can_replace_with_barrier,
                })
            }
        }
        ConfiguredFeatureKind::LargeDripstone(config) => {
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let floor_to_ceiling_search_range = config.floor_to_ceiling_search_range;
            let column_radius = generate_int_provider(&config.column_radius);
            let height_scale = generate_float_provider(config.height_scale);
            let max_column_radius_to_cave_height_ratio =
                config.max_column_radius_to_cave_height_ratio;
            let stalactite_bluntness = generate_float_provider(config.stalactite_bluntness);
            let stalagmite_bluntness = generate_float_provider(config.stalagmite_bluntness);
            let wind_speed = generate_float_provider(config.wind_speed);
            let min_radius_for_wind = config.min_radius_for_wind;
            let min_bluntness_for_wind = config.min_bluntness_for_wind;
            quote! {
                ConfiguredFeatureKind::LargeDripstone(LargeDripstoneConfiguration {
                    replaceable_blocks: #replaceable_blocks,
                    floor_to_ceiling_search_range: #floor_to_ceiling_search_range,
                    column_radius: #column_radius,
                    height_scale: #height_scale,
                    max_column_radius_to_cave_height_ratio: #max_column_radius_to_cave_height_ratio,
                    stalactite_bluntness: #stalactite_bluntness,
                    stalagmite_bluntness: #stalagmite_bluntness,
                    wind_speed: #wind_speed,
                    min_radius_for_wind: #min_radius_for_wind,
                    min_bluntness_for_wind: #min_bluntness_for_wind,
                })
            }
        }
        ConfiguredFeatureKind::MonsterRoom => quote! { ConfiguredFeatureKind::MonsterRoom },
        ConfiguredFeatureKind::MultifaceGrowth(config) => {
            let block = generate_block_ref(&config.block);
            let search_range = config.search_range;
            let can_place_on_floor = config.can_place_on_floor;
            let can_place_on_ceiling = config.can_place_on_ceiling;
            let can_place_on_wall = config.can_place_on_wall;
            let chance_of_spreading = config.chance_of_spreading;
            let can_be_placed_on = generate_vec(&config.can_be_placed_on, generate_block_ref);
            quote! {
                ConfiguredFeatureKind::MultifaceGrowth(MultifaceGrowthConfiguration {
                    block: #block,
                    search_range: #search_range,
                    can_place_on_floor: #can_place_on_floor,
                    can_place_on_ceiling: #can_place_on_ceiling,
                    can_place_on_wall: #can_place_on_wall,
                    chance_of_spreading: #chance_of_spreading,
                    can_be_placed_on: #can_be_placed_on,
                })
            }
        }
        ConfiguredFeatureKind::NetherForestVegetation(config) => {
            let state_provider = generate_block_state_provider(&config.state_provider);
            let spread_width = config.spread_width;
            let spread_height = config.spread_height;
            quote! {
                ConfiguredFeatureKind::NetherForestVegetation(NetherForestVegetationConfiguration {
                    state_provider: #state_provider,
                    spread_width: #spread_width,
                    spread_height: #spread_height,
                })
            }
        }
        ConfiguredFeatureKind::NetherrackReplaceBlobs(config) => {
            let target = generate_block_state_data(&config.target);
            let state = generate_block_state_data(&config.state);
            let radius = generate_int_provider(&config.radius);
            quote! {
                ConfiguredFeatureKind::NetherrackReplaceBlobs(NetherrackReplaceBlobsConfiguration {
                    target: #target,
                    state: #state,
                    radius: #radius,
                })
            }
        }
        ConfiguredFeatureKind::Ore(config) => {
            let targets = generate_vec(&config.targets, generate_ore_target);
            let size = config.size;
            let discard_chance_on_air_exposure = config.discard_chance_on_air_exposure;
            quote! {
                ConfiguredFeatureKind::Ore(OreConfiguration {
                    targets: #targets,
                    size: #size,
                    discard_chance_on_air_exposure: #discard_chance_on_air_exposure,
                })
            }
        }
        ConfiguredFeatureKind::PointedDripstone(config) => {
            let chance_of_taller_dripstone = config.chance_of_taller_dripstone;
            let chance_of_directional_spread = config.chance_of_directional_spread;
            let chance_of_spread_radius2 = config.chance_of_spread_radius2;
            let chance_of_spread_radius3 = config.chance_of_spread_radius3;
            quote! {
                ConfiguredFeatureKind::PointedDripstone(PointedDripstoneConfiguration {
                    chance_of_taller_dripstone: #chance_of_taller_dripstone,
                    chance_of_directional_spread: #chance_of_directional_spread,
                    chance_of_spread_radius2: #chance_of_spread_radius2,
                    chance_of_spread_radius3: #chance_of_spread_radius3,
                })
            }
        }
        ConfiguredFeatureKind::RandomBooleanSelector(config) => {
            let feature_true = generate_placed_feature_ref(&config.feature_true);
            let feature_false = generate_placed_feature_ref(&config.feature_false);
            quote! {
                ConfiguredFeatureKind::RandomBooleanSelector(RandomBooleanSelectorConfiguration {
                    feature_true: #feature_true,
                    feature_false: #feature_false,
                })
            }
        }
        ConfiguredFeatureKind::RandomSelector(config) => {
            let features = generate_vec(&config.features, generate_weighted_placed_feature);
            let default = generate_placed_feature_ref(&config.default);
            quote! {
                ConfiguredFeatureKind::RandomSelector(RandomSelectorConfiguration {
                    features: #features,
                    default: #default,
                })
            }
        }
        ConfiguredFeatureKind::WeightedRandomSelector(config) => {
            let features = generate_vec(&config.features, generate_weighted_random_placed_feature);
            quote! {
                ConfiguredFeatureKind::WeightedRandomSelector(WeightedRandomFeatureConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::RootSystem(config) => {
            let feature = generate_placed_feature_ref(&config.feature);
            let required_vertical_space_for_tree = config.required_vertical_space_for_tree;
            let level_test_distance = config.level_test_distance;
            let max_level_deviation = config.max_level_deviation;
            let root_radius = config.root_radius;
            let root_placement_attempts = config.root_placement_attempts;
            let root_column_max_height = config.root_column_max_height;
            let hanging_root_radius = config.hanging_root_radius;
            let hanging_roots_vertical_span = config.hanging_roots_vertical_span;
            let hanging_root_placement_attempts = config.hanging_root_placement_attempts;
            let allowed_vertical_water_for_tree = config.allowed_vertical_water_for_tree;
            let root_state_provider = generate_block_state_provider(&config.root_state_provider);
            let hanging_root_state_provider =
                generate_block_state_provider(&config.hanging_root_state_provider);
            let root_replaceable = generate_block_holder_set(&config.root_replaceable);
            let allowed_tree_position = generate_block_predicate(&config.allowed_tree_position);
            quote! {
                ConfiguredFeatureKind::RootSystem(RootSystemConfiguration {
                    feature: #feature,
                    required_vertical_space_for_tree: #required_vertical_space_for_tree,
                    level_test_distance: #level_test_distance,
                    max_level_deviation: #max_level_deviation,
                    root_radius: #root_radius,
                    root_placement_attempts: #root_placement_attempts,
                    root_column_max_height: #root_column_max_height,
                    hanging_root_radius: #hanging_root_radius,
                    hanging_roots_vertical_span: #hanging_roots_vertical_span,
                    hanging_root_placement_attempts: #hanging_root_placement_attempts,
                    allowed_vertical_water_for_tree: #allowed_vertical_water_for_tree,
                    root_state_provider: #root_state_provider,
                    hanging_root_state_provider: #hanging_root_state_provider,
                    root_replaceable: #root_replaceable,
                    allowed_tree_position: #allowed_tree_position,
                })
            }
        }
        ConfiguredFeatureKind::ScatteredOre(config) => {
            let targets = generate_vec(&config.targets, generate_ore_target);
            let size = config.size;
            let discard_chance_on_air_exposure = config.discard_chance_on_air_exposure;
            quote! {
                ConfiguredFeatureKind::ScatteredOre(OreConfiguration {
                    targets: #targets,
                    size: #size,
                    discard_chance_on_air_exposure: #discard_chance_on_air_exposure,
                })
            }
        }
        ConfiguredFeatureKind::SculkPatch(config) => {
            let charge_count = config.charge_count;
            let amount_per_charge = config.amount_per_charge;
            let spread_attempts = config.spread_attempts;
            let growth_rounds = config.growth_rounds;
            let spread_rounds = config.spread_rounds;
            let extra_rare_growths = generate_int_provider(&config.extra_rare_growths);
            let catalyst_chance = config.catalyst_chance;
            quote! {
                ConfiguredFeatureKind::SculkPatch(SculkPatchConfiguration {
                    charge_count: #charge_count,
                    amount_per_charge: #amount_per_charge,
                    spread_attempts: #spread_attempts,
                    growth_rounds: #growth_rounds,
                    spread_rounds: #spread_rounds,
                    extra_rare_growths: #extra_rare_growths,
                    catalyst_chance: #catalyst_chance,
                })
            }
        }
        ConfiguredFeatureKind::SeaPickle(config) => {
            let count = generate_int_provider(&config.count);
            quote! {
                ConfiguredFeatureKind::SeaPickle(SeaPickleConfiguration {
                    count: #count,
                })
            }
        }
        ConfiguredFeatureKind::Seagrass(config) => {
            let probability = config.probability;
            quote! {
                ConfiguredFeatureKind::Seagrass(SeagrassConfiguration {
                    probability: #probability,
                })
            }
        }
        ConfiguredFeatureKind::Sequence(config) => {
            let features = generate_vec(&config.features, generate_placed_feature_ref);
            quote! {
                ConfiguredFeatureKind::Sequence(CompositeFeatureConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::SimpleBlock(config) => {
            let to_place = generate_block_state_provider(&config.to_place);
            let schedule_tick = config.schedule_tick;
            quote! {
                ConfiguredFeatureKind::SimpleBlock(SimpleBlockConfiguration {
                    to_place: #to_place,
                    schedule_tick: #schedule_tick,
                })
            }
        }
        ConfiguredFeatureKind::SimpleRandomSelector(config) => {
            let features = generate_vec(&config.features, generate_placed_feature_ref);
            quote! {
                ConfiguredFeatureKind::SimpleRandomSelector(SimpleRandomSelectorConfiguration {
                    features: #features,
                })
            }
        }
        ConfiguredFeatureKind::Speleothem(config) => {
            let base_block = generate_block_state_data(&config.base_block);
            let pointed_block = generate_block_state_data(&config.pointed_block);
            let replaceable_blocks = generate_block_holder_set(&config.replaceable_blocks);
            let chance_of_taller_generation = config.chance_of_taller_generation;
            let chance_of_directional_spread = config.chance_of_directional_spread;
            let chance_of_spread_radius2 = config.chance_of_spread_radius2;
            let chance_of_spread_radius3 = config.chance_of_spread_radius3;
            quote! {
                ConfiguredFeatureKind::Speleothem(SpeleothemConfiguration {
                    base_block: #base_block,
                    pointed_block: #pointed_block,
                    replaceable_blocks: #replaceable_blocks,
                    chance_of_taller_generation: #chance_of_taller_generation,
                    chance_of_directional_spread: #chance_of_directional_spread,
                    chance_of_spread_radius2: #chance_of_spread_radius2,
                    chance_of_spread_radius3: #chance_of_spread_radius3,
                })
            }
        }
        ConfiguredFeatureKind::Spike(config) => {
            let state = generate_block_state_data(&config.state);
            let can_place_on = generate_block_predicate(&config.can_place_on);
            let can_replace = generate_block_predicate(&config.can_replace);
            quote! {
                ConfiguredFeatureKind::Spike(SpikeConfiguration {
                    state: #state,
                    can_place_on: #can_place_on,
                    can_replace: #can_replace,
                })
            }
        }
        ConfiguredFeatureKind::SpringFeature(config) => {
            let state = generate_fluid_state_data(&config.state);
            let requires_block_below = config.requires_block_below;
            let rock_count = config.rock_count;
            let hole_count = config.hole_count;
            let valid_blocks = generate_block_holder_set(&config.valid_blocks);
            quote! {
                ConfiguredFeatureKind::SpringFeature(SpringConfiguration {
                    state: #state,
                    requires_block_below: #requires_block_below,
                    rock_count: #rock_count,
                    hole_count: #hole_count,
                    valid_blocks: #valid_blocks,
                })
            }
        }
        ConfiguredFeatureKind::Template(config) => {
            let templates = generate_vec(&config.templates, generate_weighted_template_entry);
            quote! {
                ConfiguredFeatureKind::Template(TemplateFeatureConfiguration {
                    templates: #templates,
                })
            }
        }
        ConfiguredFeatureKind::Tree(config) => {
            let trunk_provider = generate_block_state_provider(&config.trunk_provider);
            let below_trunk_provider = generate_block_state_provider(&config.below_trunk_provider);
            let foliage_provider = generate_block_state_provider(&config.foliage_provider);
            let trunk_placer = generate_trunk_placer(&config.trunk_placer);
            let foliage_placer = generate_foliage_placer(&config.foliage_placer);
            let minimum_size = generate_feature_size(&config.minimum_size);
            let decorators = generate_vec(&config.decorators, generate_tree_decorator);
            let root_placer = generate_option(&config.root_placer, generate_root_placer);
            let ignore_vines = config.ignore_vines;
            quote! {
                ConfiguredFeatureKind::Tree(TreeConfiguration {
                    trunk_provider: #trunk_provider,
                    below_trunk_provider: #below_trunk_provider,
                    foliage_provider: #foliage_provider,
                    trunk_placer: #trunk_placer,
                    foliage_placer: #foliage_placer,
                    minimum_size: #minimum_size,
                    decorators: #decorators,
                    root_placer: #root_placer,
                    ignore_vines: #ignore_vines,
                })
            }
        }
        ConfiguredFeatureKind::TwistingVines(config) => {
            let spread_width = config.spread_width;
            let spread_height = config.spread_height;
            let max_height = config.max_height;
            quote! {
                ConfiguredFeatureKind::TwistingVines(TwistingVinesConfiguration {
                    spread_width: #spread_width,
                    spread_height: #spread_height,
                    max_height: #max_height,
                })
            }
        }
        ConfiguredFeatureKind::UnderwaterMagma(config) => {
            let floor_search_range = config.floor_search_range;
            let placement_radius_around_floor = config.placement_radius_around_floor;
            let placement_probability_per_valid_position =
                config.placement_probability_per_valid_position;
            quote! {
                ConfiguredFeatureKind::UnderwaterMagma(UnderwaterMagmaConfiguration {
                    floor_search_range: #floor_search_range,
                    placement_radius_around_floor: #placement_radius_around_floor,
                    placement_probability_per_valid_position: #placement_probability_per_valid_position,
                })
            }
        }
        ConfiguredFeatureKind::VegetationPatch(config) => {
            generate_vegetation_patch_kind("VegetationPatch", config)
        }
        ConfiguredFeatureKind::Vines => quote! { ConfiguredFeatureKind::Vines },
        ConfiguredFeatureKind::VoidStartPlatform => {
            quote! { ConfiguredFeatureKind::VoidStartPlatform }
        }
        ConfiguredFeatureKind::WaterloggedVegetationPatch(config) => {
            generate_vegetation_patch_kind("WaterloggedVegetationPatch", config)
        }
        ConfiguredFeatureKind::WeepingVines => quote! { ConfiguredFeatureKind::WeepingVines },
    }
}

fn generate_vegetation_patch_kind(
    variant_name: &str,
    config: &VegetationPatchConfiguration,
) -> TokenStream {
    let variant = Ident::new(variant_name, Span::call_site());
    let replaceable = generate_identifier(&config.replaceable);
    let ground_state = generate_block_state_provider(&config.ground_state);
    let vegetation_feature = generate_placed_feature_ref(&config.vegetation_feature);
    let surface = generate_vertical_surface(config.surface);
    let depth = generate_int_provider(&config.depth);
    let extra_bottom_block_chance = config.extra_bottom_block_chance;
    let vertical_range = config.vertical_range;
    let vegetation_chance = config.vegetation_chance;
    let xz_radius = generate_int_provider(&config.xz_radius);
    let extra_edge_column_chance = config.extra_edge_column_chance;
    quote! {
        ConfiguredFeatureKind::#variant(VegetationPatchConfiguration {
            replaceable: #replaceable,
            ground_state: #ground_state,
            vegetation_feature: #vegetation_feature,
            surface: #surface,
            depth: #depth,
            extra_bottom_block_chance: #extra_bottom_block_chance,
            vertical_range: #vertical_range,
            vegetation_chance: #vegetation_chance,
            xz_radius: #xz_radius,
            extra_edge_column_chance: #extra_edge_column_chance,
        })
    }
}

pub(crate) fn build_configured() -> TokenStream {
    let dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/configured_feature";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.path();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        let kind = serde_json::from_str::<ConfiguredFeatureKind>(&content)
            .unwrap_or_else(|err| panic!("failed to parse configured feature {name}: {err}"));
        entries.push((name, generate_configured_feature_kind(&kind)));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::{feature::*, vanilla_blocks, vanilla_fluids};
        use steel_utils::value_providers::{
            FloatProvider, HeightProvider, IntProvider, UniformIntProvider, VerticalAnchor,
            WeightedIntProvider,
        };
        use steel_utils::{Direction, Identifier, Rotation};
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, kind) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        stream.extend(quote! {
            pub static #ident: LazyLock<ConfiguredFeature> = LazyLock::new(|| {
                ConfiguredFeature {
                    key: Identifier::vanilla_static(#name),
                    kind: #kind,
                    id: OnceLock::new(),
                }
            });
        });
        register.extend(quote! {
            registry.register(&#ident);
        });
    }

    stream.extend(quote! {
        pub fn register_configured_features(registry: &mut ConfiguredFeatureRegistry) {
            #register
        }
    });

    stream
}

pub(crate) fn build_placed() -> TokenStream {
    let dir = "../steel-utils/build_assets/builtin_datapacks/minecraft/worldgen/placed_feature";
    println!("cargo:rerun-if-changed={dir}");

    let mut entries = Vec::new();
    for entry in sorted_json_files(dir) {
        let name = resource_name(&entry);
        let path = entry.path();
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {name}: {err}"));
        let data = serde_json::from_str::<PlacedFeatureData>(&content)
            .unwrap_or_else(|err| panic!("failed to parse placed feature {name}: {err}"));
        entries.push((name, generate_placed_feature_data(&data)));
    }

    let mut stream = TokenStream::new();
    stream.extend(quote! {
        use crate::{feature::*, vanilla_blocks, vanilla_fluids};
        use steel_utils::value_providers::{
            FloatProvider, HeightProvider, IntProvider, UniformIntProvider, VerticalAnchor,
            WeightedIntProvider,
        };
        use steel_utils::{Direction, Identifier, Rotation};
        use std::sync::{LazyLock, OnceLock};
    });

    let mut register = TokenStream::new();
    for (name, data) in &entries {
        let ident = Ident::new(&name.to_shouty_snake_case(), Span::call_site());
        stream.extend(quote! {
            pub static #ident: LazyLock<PlacedFeature> = LazyLock::new(|| {
                PlacedFeature {
                    key: Identifier::vanilla_static(#name),
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
        pub fn register_placed_features(registry: &mut PlacedFeatureRegistry) {
            #register
        }
    });

    stream
}
