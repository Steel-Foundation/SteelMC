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

mod common;
mod configured;
mod data;
mod placement;
mod providers;
mod structures;

use common::{
    generate_block_holder_set, generate_block_ref, generate_block_ref_list,
    generate_block_state_data, generate_box, generate_configured_feature_entry_ref,
    generate_direction, generate_fluid_ref_list, generate_fluid_state_data, generate_identifier,
    generate_offset, generate_option, generate_placed_feature_entry_ref, generate_rotation,
    generate_vec, generate_vertical_anchor, resource_name, sorted_json_files,
};
use configured::generate_configured_feature_kind;
use placement::{
    generate_block_predicate, generate_placed_feature_data, generate_placed_feature_ref,
};
use providers::{
    generate_block_state_provider, generate_float_provider, generate_height_provider,
    generate_int_provider, generate_uniform_int_provider,
};
use structures::{
    generate_block_column_layer, generate_end_spike, generate_feature_size,
    generate_foliage_placer, generate_geode_block_settings, generate_geode_crack_settings,
    generate_geode_layer_settings, generate_huge_mushroom_kind, generate_ore_target,
    generate_root_placer, generate_tree_decorator, generate_trunk_placer,
    generate_vertical_surface, generate_weighted_placed_feature,
    generate_weighted_random_placed_feature, generate_weighted_template_entry,
};

use data::{
    AboveRootPlacement, BlobFoliagePlacer, BlockColumnLayer, BlockHolderSet, BlockPredicate,
    BlockStateData, BlockStateProvider, ConfiguredFeatureKind, ConfiguredFeatureRef,
    DualNoiseProvider, EndSpike, FeatureHeightmap, FeatureNoiseParameters, FeatureSize,
    FluidStateData, FoliagePlacer, FoliagePlacerBase, GeodeBlockSettings, GeodeCrackSettings,
    GeodeLayerSettings, HugeMushroomConfiguration, IdentifierList, MangroveRootPlacement,
    NoiseProvider, NoiseThresholdProvider, OreTarget, PlacedFeatureData, PlacedFeatureRef,
    PlacementModifier, RootPlacer, RuleBasedStateProviderRule, RuleTest, TemplateEntry,
    TreeDecorator, TrunkPlacer, TrunkPlacerBase, VegetationPatchConfiguration, VerticalSurface,
    WeightedBlockState, WeightedPlacedFeature, WeightedRandomPlacedFeature, WeightedTemplateEntry,
};

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
        use glam::IVec3;
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
        use glam::IVec3;
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
