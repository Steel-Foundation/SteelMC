use std::slice;

use super::*;
use steel_registry::blocks::properties::{DoorHingeSide, SlabType};
use steel_registry::test_support::init_test_registry;
use steel_registry::vanilla_entities;

fn test_registry() -> Registry {
    init_test_registry();
    Registry::new_vanilla()
}

#[test]
fn palette_blocks_skips_all_out_of_bounds_for_current_chunk_processors() {
    let blocks = [StructureBlockInfo {
        pos: BlockPos::new(32, 0, 0),
        state: BlockStateId(0),
        nbt: None,
    }];
    let settings = StructurePlaceSettings {
        mirror: StructureMirror::None,
        rotation: Rotation::None,
        rotation_pivot: BlockPos::ZERO,
        bounding_box: BoundingBox::new(IVec3::ZERO, IVec3::new(15, 255, 15)),
        processors: &[],
        block_ignore: StructureBlockIgnore::None,
        late_block_ignore: StructureBlockIgnore::None,
        replace_jigsaws: false,
        projection: None,
        processor_random: StructureProcessorRandom::Positional,
        liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
    };
    let mut processed = 0;

    StructureTemplate::palette_blocks_for_placement(&blocks, BlockPos::ZERO, &settings, |_, _| {
        processed += 1;
    });

    assert_eq!(processed, 0);
}

#[test]
fn palette_blocks_keeps_out_of_bounds_for_capped_processors() {
    let blocks = [StructureBlockInfo {
        pos: BlockPos::new(32, 0, 0),
        state: BlockStateId(0),
        nbt: None,
    }];
    let capped = StructureProcessorKind::Capped {
        delegate: Box::new(StructureProcessorKind::LavaSubmergedBlock),
        limit: IntProvider::Constant(1),
    };
    let settings = StructurePlaceSettings {
        mirror: StructureMirror::None,
        rotation: Rotation::None,
        rotation_pivot: BlockPos::ZERO,
        bounding_box: BoundingBox::new(IVec3::ZERO, IVec3::new(15, 255, 15)),
        processors: slice::from_ref(&capped),
        block_ignore: StructureBlockIgnore::None,
        late_block_ignore: StructureBlockIgnore::None,
        replace_jigsaws: false,
        projection: None,
        processor_random: StructureProcessorRandom::Positional,
        liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
    };
    let mut processed = 0;
    let mut processed_pos = None;

    StructureTemplate::palette_blocks_for_placement(
        &blocks,
        BlockPos::ZERO,
        &settings,
        |_, pos| {
            processed += 1;
            processed_pos = Some(pos);
        },
    );

    assert_eq!(processed, 1);
    assert_eq!(processed_pos, Some(BlockPos::new(32, 0, 0)));
}

#[test]
fn zero_position_with_transform_matches_vanilla_rotation_offsets() {
    let template = StructureTemplate {
        size: IVec3::new(6, 10, 8),
        palettes: Vec::new(),
        entities: Vec::new(),
    };
    let zero = BlockPos::new(100, 64, 200);

    assert_eq!(
        template.zero_position_with_transform(zero, Rotation::None),
        zero
    );
    assert_eq!(
        template.zero_position_with_transform(zero, Rotation::Clockwise90),
        BlockPos::new(107, 64, 200)
    );
    assert_eq!(
        template.zero_position_with_transform(zero, Rotation::Clockwise180),
        BlockPos::new(105, 64, 207)
    );
    assert_eq!(
        template.zero_position_with_transform(zero, Rotation::CounterClockwise90),
        BlockPos::new(100, 64, 205)
    );
}

#[test]
fn bounding_box_with_transform_matches_vanilla_mirror_rotation_pivot() {
    let template = StructureTemplate {
        size: IVec3::new(6, 10, 8),
        palettes: Vec::new(),
        entities: Vec::new(),
    };

    assert_eq!(
        template.bounding_box_with_transform(
            BlockPos::new(100, 64, 200),
            Rotation::Clockwise90,
            StructureMirror::FrontBack,
            BlockPos::new(2, 0, 3),
        ),
        BoundingBox::new(IVec3::new(98, 64, 196), IVec3::new(105, 73, 201))
    );
    assert_eq!(
        template.bounding_box_with_transform(
            BlockPos::new(100, 64, 200),
            Rotation::CounterClockwise90,
            StructureMirror::LeftRight,
            BlockPos::new(2, 0, 3),
        ),
        BoundingBox::new(IVec3::new(92, 64, 200), IVec3::new(99, 73, 205))
    );
}

#[test]
fn block_pos_seed_matches_vanilla_mth_get_seed() {
    assert_eq!(
        StructureTemplate::block_pos_seed(BlockPos::new(12, -3, 45)),
        103_080_484_998_711
    );
}

#[test]
fn village_template_loads_entity_payloads() {
    let registry = test_registry();
    let template = StructureTemplate::load_vanilla(
        &registry,
        &Identifier::vanilla_static("village/plains/villagers/unemployed"),
    )
    .expect("villager template should be bundled");

    assert_eq!(template.entities.len(), 1);
    assert_eq!(
        &template.entities[0].entity_type.key,
        &vanilla_entities::VILLAGER.key
    );
    assert!(template.entities[0].nbt.contains("VillagerData"));
    assert!(!template.entities[0].nbt.contains("id"));
}

#[test]
fn brushable_append_loot_infers_block_entity_without_container_reseed() {
    let registry = test_registry();
    let suspicious_sand = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::SUSPICIOUS_SAND);
    let mut brushable_nbt = NbtCompound::new();
    brushable_nbt.insert("LootTable", "minecraft:archaeology/ocean_ruin_warm");
    brushable_nbt.insert("LootTableSeed", 42_i64);

    let brushable_type = StructureTemplate::block_entity_type_for_nbt_or_state(
        &registry,
        suspicious_sand,
        &brushable_nbt,
    )
    .expect("suspicious sand should infer brushable block entity");

    assert_eq!(
        &brushable_type.key,
        &vanilla_block_entity_types::BRUSHABLE_BLOCK.key
    );
    assert!(!StructureTemplate::should_reseed_template_loot(
        Some(brushable_type),
        &brushable_nbt
    ));

    let mut chest_nbt = NbtCompound::new();
    chest_nbt.insert("id", "minecraft:chest");
    chest_nbt.insert("LootTable", "minecraft:chests/village/village_weaponsmith");
    let chest_type = StructureTemplate::block_entity_type_for_nbt_or_state(
        &registry,
        registry.blocks.get_default_state_id(&vanilla_blocks::CHEST),
        &chest_nbt,
    )
    .expect("chest nbt should resolve block entity type");

    assert!(StructureTemplate::should_reseed_template_loot(
        Some(chest_type),
        &chest_nbt
    ));
}

#[test]
fn entity_position_and_rotation_transform_match_vanilla_offsets() {
    let pos = DVec3::new(1.25, 2.0, 3.75);
    let pivot = BlockPos::new(2, 0, 3);

    assert_eq!(
        StructureTemplate::transform_entity_position(
            pos,
            StructureMirror::FrontBack,
            Rotation::Clockwise90,
            pivot,
        ),
        DVec3::new(2.25, 2.0, 0.75)
    );
    assert_eq!(
        StructureTemplate::transform_entity_rotation(
            (30.0, 10.0),
            StructureMirror::LeftRight,
            Rotation::Clockwise90,
        ),
        (240.0, 10.0)
    );
    assert_eq!(
        StructureTemplate::transform_entity_rotation(
            (30.0, 10.0),
            StructureMirror::FrontBack,
            Rotation::Clockwise90,
        ),
        (60.0, 10.0)
    );
}

#[test]
fn hanging_entity_facing_applies_rotation_before_mirror() {
    let mut nbt = NbtCompound::new();
    nbt.insert(
        "Facing",
        StructureTemplate::entity_facing_value(Direction::North),
    );

    StructureTemplate::transform_entity_additional_nbt(
        &mut nbt,
        StructureMirror::LeftRight,
        Rotation::Clockwise90,
    );

    assert_eq!(
        nbt.byte("Facing"),
        Some(StructureTemplate::entity_facing_value(Direction::East))
    );
}

#[test]
fn mirrored_door_transform_toggles_hinge() {
    let registry = test_registry();
    let door = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::SPRUCE_DOOR);
    let door = registry.blocks.set_property(
        door,
        &BlockStateProperties::HORIZONTAL_FACING,
        Direction::East,
    );
    let door =
        registry
            .blocks
            .set_property(door, &BlockStateProperties::DOOR_HINGE, DoorHingeSide::Left);

    let mirrored = StructureTemplate::transform_state(
        &registry,
        door,
        StructureMirror::FrontBack,
        Rotation::None,
    );

    assert_eq!(
        registry
            .blocks
            .try_get_property(mirrored, &BlockStateProperties::HORIZONTAL_FACING),
        Some(Direction::West),
    );
    assert_eq!(
        registry
            .blocks
            .try_get_property(mirrored, &BlockStateProperties::DOOR_HINGE),
        Some(DoorHingeSide::Right),
    );
}

#[test]
fn jigsaw_replacement_uses_final_state_and_removes_nbt() {
    let registry = test_registry();
    let mut nbt = NbtCompound::new();
    nbt.insert(
        "final_state",
        NbtTag::String("minecraft:oak_stairs[facing=east,half=top]".into()),
    );
    let current = ProcessedBlockInfo {
        template_pos: BlockPos::ZERO,
        world_pos: BlockPos::new(1, 2, 3),
        state: registry
            .blocks
            .get_default_state_id(&vanilla_blocks::JIGSAW),
        nbt: Some(nbt),
    };

    let replaced = StructureTemplate::replace_jigsaw_block(&registry, current)
        .expect("non-structure-void final state should remain");

    assert_eq!(replaced.nbt, None);
    assert_eq!(
        replaced.state,
        StructureTemplate::parse_block_state_string(
            &registry,
            "minecraft:oak_stairs[facing=east,half=top]"
        )
        .expect("test final state should parse")
    );
}

#[test]
fn jigsaw_replacement_accepts_trailing_text_like_vanilla_parser() {
    let registry = test_registry();
    let final_state =
        "minecraft:acacia_fence[east=false,north=false,south=false,waterlogged=false,west=false]]";
    let expected =
        "minecraft:acacia_fence[east=false,north=false,south=false,waterlogged=false,west=false]";

    assert_eq!(
        StructureTemplate::parse_block_state_string(&registry, final_state),
        StructureTemplate::parse_block_state_string(&registry, expected)
    );
}

#[test]
fn jigsaw_replacement_drops_structure_void_final_state() {
    let registry = test_registry();
    let mut nbt = NbtCompound::new();
    nbt.insert(
        "final_state",
        NbtTag::String("minecraft:structure_void".into()),
    );
    let current = ProcessedBlockInfo {
        template_pos: BlockPos::ZERO,
        world_pos: BlockPos::new(1, 2, 3),
        state: registry
            .blocks
            .get_default_state_id(&vanilla_blocks::JIGSAW),
        nbt: Some(nbt),
    };

    assert!(StructureTemplate::replace_jigsaw_block(&registry, current).is_none());
}

#[test]
fn structure_block_ignore_modes_match_vanilla_single_variants() {
    let registry = test_registry();
    let structure_block = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::STRUCTURE_BLOCK);
    let air = registry.blocks.get_default_state_id(&vanilla_blocks::AIR);
    let stone = registry.blocks.get_default_state_id(&vanilla_blocks::STONE);

    assert!(StructureBlockIgnore::StructureBlock.ignores(&registry, structure_block));
    assert!(!StructureBlockIgnore::StructureBlock.ignores(&registry, air));
    assert!(StructureBlockIgnore::StructureAndAir.ignores(&registry, structure_block));
    assert!(StructureBlockIgnore::StructureAndAir.ignores(&registry, air));
    assert!(!StructureBlockIgnore::StructureAndAir.ignores(&registry, stone));
}

#[test]
fn block_age_processor_preserves_slab_properties() {
    let registry = test_registry();
    let slab = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB);
    let slab = registry
        .blocks
        .set_property(slab, &BlockStateProperties::SLAB_TYPE, SlabType::Top);
    let current = ProcessedBlockInfo {
        template_pos: BlockPos::ZERO,
        world_pos: BlockPos::new(12, 70, -4),
        state: slab,
        nbt: None,
    };
    let mut random = LegacyRandom::from_seed(1);

    let processed =
        StructureTemplate::process_block_age_with_random(&registry, current, 1.0, &mut random);

    assert_eq!(
        StructureTemplate::block_for_state(&registry, processed.state),
        &vanilla_blocks::MOSSY_STONE_BRICK_SLAB
    );
    assert_eq!(
        registry
            .blocks
            .try_get_property(processed.state, &BlockStateProperties::SLAB_TYPE),
        Some(SlabType::Top),
    );
}

#[test]
fn lava_submerged_processor_keeps_non_full_blocks_as_lava() {
    let registry = test_registry();
    let slab = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::STONE_BRICK_SLAB);
    let current = ProcessedBlockInfo {
        template_pos: BlockPos::ZERO,
        world_pos: BlockPos::new(0, 64, 0),
        state: slab,
        nbt: None,
    };
    let lava = registry.blocks.get_default_state_id(&vanilla_blocks::LAVA);

    let processed = StructureTemplate::process_lava_submerged_block(&registry, lava, current);

    assert_eq!(
        StructureTemplate::block_for_state(&registry, processed.state),
        &vanilla_blocks::LAVA
    );
}

#[test]
fn blackstone_replace_processor_preserves_stair_orientation() {
    let registry = test_registry();
    let stairs = registry
        .blocks
        .get_default_state_id(&vanilla_blocks::STONE_BRICK_STAIRS);
    let stairs =
        registry
            .blocks
            .set_property(stairs, &BlockStateProperties::FACING, Direction::East);
    let stairs = registry
        .blocks
        .set_property(stairs, &BlockStateProperties::HALF, Half::Top);
    let current = ProcessedBlockInfo {
        template_pos: BlockPos::ZERO,
        world_pos: BlockPos::new(0, 64, 0),
        state: stairs,
        nbt: None,
    };

    let processed = StructureTemplate::process_blackstone_replace(&registry, current);

    assert_eq!(
        StructureTemplate::block_for_state(&registry, processed.state),
        &vanilla_blocks::POLISHED_BLACKSTONE_BRICK_STAIRS,
    );
    assert_eq!(
        registry
            .blocks
            .try_get_property(processed.state, &BlockStateProperties::FACING),
        Some(Direction::East),
    );
    assert_eq!(
        registry
            .blocks
            .try_get_property(processed.state, &BlockStateProperties::HALF),
        Some(Half::Top),
    );
}

#[test]
fn data_markers_read_shipwreck_structure_blocks() {
    let registry = test_registry();
    let template = StructureTemplate::load_vanilla(
        &registry,
        &Identifier::vanilla_static("shipwreck/with_mast"),
    )
    .expect("shipwreck template should be bundled");
    let settings = StructurePlaceSettings {
        mirror: StructureMirror::None,
        rotation: Rotation::Clockwise90,
        rotation_pivot: BlockPos::new(4, 0, 15),
        bounding_box: BoundingBox::new(IVec3::new(-64, 0, -64), IVec3::new(64, 128, 64)),
        processors: &[],
        block_ignore: StructureBlockIgnore::StructureAndAir,
        late_block_ignore: StructureBlockIgnore::None,
        replace_jigsaws: false,
        projection: None,
        processor_random: StructureProcessorRandom::Positional,
        liquid_settings: LiquidSettingsData::ApplyWaterlogging,
    };
    let mut random = WorldgenRandom::from_seed(0);

    let mut markers = template
        .data_markers(&registry, BlockPos::ZERO, &settings, &mut random)
        .into_iter()
        .map(|marker| marker.metadata)
        .collect::<Vec<_>>();
    markers.sort();

    assert_eq!(markers, ["map_chest", "supply_chest", "treasure_chest"]);
}

#[test]
fn data_markers_read_igloo_chest_structure_block() {
    let registry = test_registry();
    let template =
        StructureTemplate::load_vanilla(&registry, &Identifier::vanilla_static("igloo/bottom"))
            .expect("igloo bottom template should be bundled");
    let settings = StructurePlaceSettings {
        mirror: StructureMirror::None,
        rotation: Rotation::Clockwise180,
        rotation_pivot: BlockPos::new(3, 6, 7),
        bounding_box: BoundingBox::new(IVec3::new(-64, 0, -64), IVec3::new(64, 128, 64)),
        processors: &[],
        block_ignore: StructureBlockIgnore::StructureBlock,
        late_block_ignore: StructureBlockIgnore::None,
        replace_jigsaws: false,
        projection: None,
        processor_random: StructureProcessorRandom::Positional,
        liquid_settings: LiquidSettingsData::IgnoreWaterlogging,
    };
    let mut random = WorldgenRandom::from_seed(0);

    let markers = template
        .data_markers(&registry, BlockPos::ZERO, &settings, &mut random)
        .into_iter()
        .map(|marker| marker.metadata)
        .collect::<Vec<_>>();

    assert_eq!(markers, ["chest"]);
}
