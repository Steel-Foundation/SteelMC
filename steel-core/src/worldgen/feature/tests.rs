use std::collections::BTreeMap;

use steel_registry::Registry;

use super::runner::FeatureDecorationRunner;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::feature::FluidStateData;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::Identifier;

use crate::worldgen::BiomeSourceKind;

#[test]
fn feature_direction_order_matches_java_direction_values() {
    assert_eq!(
        FeatureDecorationRunner::VANILLA_DIRECTION_VALUES,
        [
            steel_utils::Direction::Down,
            steel_utils::Direction::Up,
            steel_utils::Direction::North,
            steel_utils::Direction::South,
            steel_utils::Direction::West,
            steel_utils::Direction::East,
        ]
    );
}

#[test]
fn vanilla_feature_sorter_builds_for_all_builtin_biome_sources() {
    let mut registry = Registry::new_vanilla();
    registry.freeze();

    let sources = [
        BiomeSourceKind::overworld(0),
        BiomeSourceKind::nether(0),
        BiomeSourceKind::end(0),
    ];

    for source in sources {
        let possible_biomes = source.possible_biome_refs();
        let runner = FeatureDecorationRunner::new(&possible_biomes, &registry);
        assert!(runner.sorter.step_count() > 0);
    }
}

#[test]
fn block_column_truncation_matches_vanilla_tip_priority() {
    let mut preserved_base = [2, 3, 4];
    FeatureDecorationRunner::truncate_block_column_layers(&mut preserved_base, 9, 6, false);
    assert_eq!(preserved_base, [2, 3, 1]);

    let mut preserved_tip = [2, 3, 4];
    FeatureDecorationRunner::truncate_block_column_layers(&mut preserved_tip, 9, 6, true);
    assert_eq!(preserved_tip, [0, 2, 4]);
}

#[test]
fn spring_source_fluid_state_creates_vanilla_legacy_source_block() {
    let mut registry = Registry::new_vanilla();
    registry.freeze();

    let data = FluidStateData {
        name: Identifier::vanilla_static("water"),
        properties: BTreeMap::from([("falling".to_owned(), "true".to_owned())]),
    };

    let fluid_state = FeatureDecorationRunner::fluid_state_from_data(&registry, &data);
    assert_eq!(fluid_state.fluid_id, &vanilla_fluids::WATER);
    assert_eq!(fluid_state.amount, 8);
    assert!(fluid_state.falling);

    let block_state =
        FeatureDecorationRunner::legacy_block_from_fluid_state(&registry, fluid_state);
    assert_eq!(
        registry.blocks.by_state_id(block_state),
        Some(&vanilla_blocks::WATER)
    );
    assert_eq!(
        registry
            .blocks
            .get_property(block_state, &BlockStateProperties::LEVEL),
        0
    );
}

#[test]
fn scattered_ore_offset_rounding_matches_java_math_round() {
    assert_eq!(FeatureDecorationRunner::java_round_f32(-1.5), -1);
    assert_eq!(FeatureDecorationRunner::java_round_f32(-0.5), 0);
    assert_eq!(FeatureDecorationRunner::java_round_f32(0.49), 0);
    assert_eq!(FeatureDecorationRunner::java_round_f32(0.5), 1);
    assert_eq!(FeatureDecorationRunner::java_round_f32(1.5), 2);
}

#[test]
fn blue_ice_horizontal_spread_radius_uses_java_integer_division() {
    assert_eq!(FeatureDecorationRunner::blue_ice_xz_diff(-5), 1);
    assert_eq!(FeatureDecorationRunner::blue_ice_xz_diff(-4), 1);
    assert_eq!(FeatureDecorationRunner::blue_ice_xz_diff(-1), 3);
    assert_eq!(FeatureDecorationRunner::blue_ice_xz_diff(1), 3);
    assert_eq!(FeatureDecorationRunner::blue_ice_xz_diff(2), 3);
}
