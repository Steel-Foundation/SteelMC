use super::{
    BlockPredicate, ConfiguredFeatureRef, FeatureHeightmap, PlacedFeatureData, PlacedFeatureRef,
    PlacementModifier, TokenStream, generate_block_ref_list, generate_block_state_data,
    generate_box, generate_configured_feature_entry_ref, generate_configured_feature_kind,
    generate_direction, generate_fluid_ref_list, generate_height_provider, generate_identifier,
    generate_int_provider, generate_offset, generate_option, generate_placed_feature_entry_ref,
    generate_vec, quote,
};

pub(super) fn generate_block_predicate(predicate: &BlockPredicate) -> TokenStream {
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

pub(super) fn generate_configured_feature_ref(feature: &ConfiguredFeatureRef) -> TokenStream {
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

pub(super) fn generate_placed_feature_ref(feature: &PlacedFeatureRef) -> TokenStream {
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

pub(super) fn generate_placed_feature_data(data: &PlacedFeatureData) -> TokenStream {
    let feature = generate_configured_feature_ref(&data.feature);
    let placement = generate_vec(&data.placement, generate_placement_modifier);
    quote! {
        PlacedFeatureData {
            feature: #feature,
            placement: #placement,
        }
    }
}

pub(super) fn generate_feature_heightmap(heightmap: FeatureHeightmap) -> TokenStream {
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

pub(super) fn generate_placement_modifier(modifier: &PlacementModifier) -> TokenStream {
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
