use glam::IVec3;
use steel_registry::Registry;

use super::runner::FeatureDecorationRunner;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::feature::FluidStateData;
use steel_registry::structure::TerrainAdjustment;
use steel_registry::{vanilla_blocks, vanilla_fluids};
use steel_utils::random::{Random as _, worldgen_random::WorldgenRandom};
use steel_utils::{BlockPos, BoundingBox, ChunkPos, Identifier};

use steel_worldgen::biomes::BiomeSourceKind;
use steel_worldgen::structure::{
    StructurePiece, StructureReferenceMap, StructureReferenceSet, StructureStart,
};

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
fn feature_horizontal_direction_order_matches_java_plane_horizontal() {
    assert_eq!(
        FeatureDecorationRunner::VANILLA_HORIZONTAL_DIRECTIONS,
        [
            steel_utils::Direction::North,
            steel_utils::Direction::East,
            steel_utils::Direction::South,
            steel_utils::Direction::West,
        ]
    );
}

#[test]
fn within_manhattan_iteration_starts_in_vanilla_order() {
    let mut positions = Vec::new();
    FeatureDecorationRunner::for_each_vanilla_within_manhattan(
        BlockPos::new(10, 20, 30),
        1,
        1,
        1,
        |pos| {
            positions.push(pos);
            positions.len() < 7
        },
    );

    assert_eq!(
        positions,
        [
            BlockPos::new(10, 20, 30),
            BlockPos::new(9, 20, 30),
            BlockPos::new(10, 19, 30),
            BlockPos::new(10, 20, 31),
            BlockPos::new(10, 20, 29),
            BlockPos::new(10, 21, 30),
            BlockPos::new(11, 20, 30),
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
fn structures_for_decoration_step_use_registry_order_inside_vanilla_step() {
    let mut registry = Registry::new_vanilla();
    registry.freeze();

    let underground = FeatureDecorationRunner::structures_for_decoration_step(&registry, 3);
    let surface = FeatureDecorationRunner::structures_for_decoration_step(&registry, 4);
    let underground_decoration =
        FeatureDecorationRunner::structures_for_decoration_step(&registry, 7);

    assert_eq!(
        underground
            .iter()
            .map(|structure| structure.key.path.as_ref())
            .collect::<Vec<_>>(),
        [
            "buried_treasure",
            "mineshaft",
            "mineshaft_mesa",
            "trail_ruins",
            "trial_chambers",
        ]
    );
    assert_eq!(
        surface
            .iter()
            .map(|structure| structure.key.path.as_ref())
            .collect::<Vec<_>>(),
        [
            "bastion_remnant",
            "desert_pyramid",
            "end_city",
            "igloo",
            "jungle_pyramid",
            "mansion",
            "monument",
            "ocean_ruin_cold",
            "ocean_ruin_warm",
            "pillager_outpost",
            "ruined_portal",
            "ruined_portal_desert",
            "ruined_portal_jungle",
            "ruined_portal_mountain",
            "ruined_portal_nether",
            "ruined_portal_ocean",
            "ruined_portal_swamp",
            "shipwreck",
            "shipwreck_beached",
            "stronghold",
            "swamp_hut",
            "village_desert",
            "village_plains",
            "village_savanna",
            "village_snowy",
            "village_taiga",
        ]
    );
    assert_eq!(
        underground_decoration
            .iter()
            .map(|structure| structure.key.path.as_ref())
            .collect::<Vec<_>>(),
        ["ancient_city", "fortress", "nether_fossil"]
    );

    assert!(
        underground
            .iter()
            .all(|structure| structure.step.decoration_ordinal() == 3)
    );
    assert!(
        surface
            .iter()
            .all(|structure| structure.step.decoration_ordinal() == 4)
    );
    assert!(
        underground_decoration
            .iter()
            .all(|structure| structure.step.decoration_ordinal() == 7)
    );
    assert!(FeatureDecorationRunner::structures_for_decoration_step(&registry, 0).is_empty());
}

#[test]
fn structure_start_resolution_uses_vanilla_reference_order_and_filters_invalid_starts() {
    let structure_id = Identifier::vanilla_static("village_plains");
    let other_id = Identifier::vanilla_static("mineshaft");
    let first = ChunkPos::new(3, 5);
    let second = ChunkPos::new(4, 5);
    let empty = ChunkPos::new(5, 5);
    let mismatched = ChunkPos::new(6, 5);
    let other = ChunkPos::new(7, 5);

    let mut references = StructureReferenceMap::default();
    let positions = references.entry(structure_id.clone()).or_default();
    positions.insert(first);
    positions.insert(second);
    positions.insert(first);
    positions.insert(empty);
    positions.insert(mismatched);
    references.entry(other_id).or_default().insert(other);

    let mut lookup_order = Vec::new();
    let starts = FeatureDecorationRunner::resolve_structure_starts_from_references(
        &references,
        &structure_id,
        |source_pos, id| {
            lookup_order.push(source_pos);
            if source_pos == empty {
                return Some(StructureStart::new(
                    id.clone(),
                    source_pos,
                    Vec::new(),
                    TerrainAdjustment::None,
                ));
            }

            let start_pos = if source_pos == mismatched {
                ChunkPos::new(99, 99)
            } else {
                source_pos
            };
            Some(StructureStart::new(
                id.clone(),
                start_pos,
                vec![StructurePiece::non_jigsaw(
                    Identifier::vanilla_static("test_piece"),
                    BoundingBox::new(IVec3::ZERO, IVec3::ZERO),
                    0,
                    None,
                )],
                TerrainAdjustment::None,
            ))
        },
    );

    assert_eq!(lookup_order, [empty, first, second, mismatched]);
    assert_eq!(
        starts
            .iter()
            .map(|start| start.chunk_pos)
            .collect::<Vec<_>>(),
        [first, second]
    );
}

#[test]
fn structure_reference_set_iterates_like_vanilla_long_open_hash_set() {
    let first = ChunkPos::new(-349_429, 434_509);
    let second = ChunkPos::new(-349_428, 434_514);
    let third = ChunkPos::new(-349_423, 434_513);

    let references: StructureReferenceSet = [first, second, third].into_iter().collect();

    assert_eq!(
        references.iter().copied().collect::<Vec<_>>(),
        [third, second, first]
    );
    assert_eq!(
        references
            .insertion_order_iter()
            .copied()
            .collect::<Vec<_>>(),
        [first, second, third]
    );
}

#[test]
fn structure_step_seed_uses_vanilla_feature_seed_shape() {
    let decoration_seed = 4_567_890_i64;
    let mut actual = WorldgenRandom::from_seed(0);
    FeatureDecorationRunner::set_structure_seed(&mut actual, decoration_seed, 2, 7);

    let mut expected = WorldgenRandom::from_seed(0);
    expected.set_feature_seed(decoration_seed, 2, 7);

    assert_eq!(actual.next_i32(), expected.next_i32());
}

#[test]
fn structure_piece_clip_box_is_center_chunk_build_height_box() {
    assert_eq!(
        FeatureDecorationRunner::chunk_writable_box(ChunkPos::new(-2, 3), -64, 320),
        BoundingBox::new(IVec3::new(-32, -63, 48), IVec3::new(-17, 319, 63))
    );
}

#[test]
fn structure_start_reference_pos_uses_first_piece_center_and_min_y() {
    let start = StructureStart::new(
        Identifier::vanilla_static("test_structure"),
        ChunkPos::new(0, 0),
        vec![
            StructurePiece::non_jigsaw(
                Identifier::vanilla_static("first_piece"),
                BoundingBox::new(IVec3::new(10, 42, 20), IVec3::new(19, 50, 29)),
                0,
                None,
            ),
            StructurePiece::non_jigsaw(
                Identifier::vanilla_static("second_piece"),
                BoundingBox::new(IVec3::new(100, 5, 100), IVec3::new(110, 9, 110)),
                1,
                None,
            ),
        ],
        TerrainAdjustment::None,
    );

    assert_eq!(
        start.placement_reference_pos(),
        Some(BlockPos::new(15, 42, 25))
    );
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
        fluid: &vanilla_fluids::WATER,
        properties: &[("falling", "true")],
    };

    let fluid_state = FeatureDecorationRunner::fluid_state_from_data(&data);
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
