use glam::{DVec3, IVec3};

use crate::direction::Direction;

use super::{
    BlockPos, ChunkPos, Identifier, PackedBlockPos, PackedChunkLocalXZ, PackedChunkPos,
    PackedSectionBlockPos, PackedSectionPos, SectionPos,
};

#[test]
fn identifier_parsing_matches_vanilla_default_namespace_rules() {
    for raw in ["stone", ":stone", "minecraft:stone"] {
        assert_eq!(
            raw.parse::<Identifier>().expect("identifier should parse"),
            Identifier::vanilla_static("stone")
        );
    }
    assert_eq!(
        "steel:example"
            .parse::<Identifier>()
            .expect("namespaced identifier should parse"),
        Identifier::new_static("steel", "example")
    );
}

#[test]
fn identifier_parsing_rejects_vanilla_invalid_names() {
    for raw in ["..:stone", "Minecraft:stone", "minecraft:bad:path"] {
        assert!(raw.parse::<Identifier>().is_err(), "{raw}");
    }
}

#[test]
fn test_block_pos_roundtrip() {
    let positions = vec![
        BlockPos(IVec3::new(0, -61, -2)),
        BlockPos(IVec3::new(0, 0, 0)),
        BlockPos(IVec3::new(100, 64, -100)),
        BlockPos(IVec3::new(-1000, -64, 1000)),
        BlockPos(IVec3::new(33_554_431, 2047, 33_554_431)), // Max positive values
        BlockPos(IVec3::new(-33_554_432, -2048, -33_554_432)), // Max negative values
    ];

    for pos in positions {
        let encoded = PackedBlockPos::from(pos);
        let decoded = encoded.to_block_pos();
        assert_eq!(
            pos, decoded,
            "Roundtrip failed for {pos:?}: encoded={encoded:?}, decoded={decoded:?}"
        );
    }
}

#[test]
fn test_block_pos_specific_case() {
    // Test the specific case from the bug report
    let pos = BlockPos(IVec3::new(0, -61, -2));
    let encoded = PackedBlockPos::from(pos);
    let decoded = encoded.to_block_pos();
    assert_eq!(pos, decoded, "Position 0, -61, -2 failed roundtrip");
}

#[test]
fn block_pos_within_manhattan_starts_in_vanilla_order() {
    let positions: Vec<_> = BlockPos::new(10, 20, 30)
        .within_manhattan(1, 1, 1)
        .take(7)
        .collect();

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
fn block_pos_spiral_around_radius_zero_returns_center() {
    let center = BlockPos::new(10, 20, 30);

    assert_eq!(
        BlockPos::spiral_around(center, 0, Direction::East, Direction::South).collect::<Vec<_>>(),
        [center]
    );
}

#[test]
fn block_pos_spiral_around_matches_vanilla_order() {
    let center = BlockPos::new(10, 20, 30);

    assert_eq!(
        BlockPos::spiral_around(center, 1, Direction::East, Direction::South).collect::<Vec<_>>(),
        [
            center,
            center.east(),
            center.east().south(),
            center.south(),
            center.west().south(),
            center.west(),
            center.west().north(),
            center.north(),
            center.east().north(),
        ]
    );
}

#[test]
fn block_pos_find_closest_match_uses_vanilla_order() {
    let origin = BlockPos::new(10, 20, 30);

    let found =
        origin.find_closest_match(1, 1, |pos| pos == origin.south() || pos == origin.west());

    assert_eq!(found, Some(origin.west()));
}

#[test]
fn packed_chunk_local_xz_masks_absolute_coordinates() {
    let packed = PackedChunkLocalXZ::from_block_pos(BlockPos::new(17, 64, 18));

    assert_eq!(packed.as_u8(), 0x12);
    assert_eq!(packed.x(), 1);
    assert_eq!(packed.z(), 2);
}

#[test]
fn packed_chunk_local_xz_rejects_invalid_local_coordinates() {
    assert!(PackedChunkLocalXZ::from_local_xz(15, 15).is_some());
    assert!(PackedChunkLocalXZ::from_local_xz(16, 0).is_none());
    assert!(PackedChunkLocalXZ::from_local_xz(0, 16).is_none());
}

#[test]
fn entity_positions_floor_before_chunk_and_section_conversion() {
    let pos = DVec3::new(-4352.5, -16.5, -4405.5);

    assert_eq!(BlockPos::from(pos), BlockPos::new(-4353, -17, -4406));
    assert_eq!(ChunkPos::from_entity_pos(pos), ChunkPos::new(-273, -276));
    assert_eq!(
        SectionPos::from_entity_pos(pos),
        SectionPos::new(-273, -2, -276)
    );
}

#[test]
fn packed_section_block_pos_masks_absolute_coordinates() {
    let packed = PackedSectionBlockPos::from_block_pos(BlockPos::new(17, -1, 18));

    assert_eq!(packed.as_u16(), 0x12f);
    assert_eq!(packed.x(), 1);
    assert_eq!(packed.y(), 15);
    assert_eq!(packed.z(), 2);
}

#[test]
fn packed_section_block_pos_rejects_invalid_raw_bits() {
    assert!(PackedSectionBlockPos::from_raw(0x0fff).is_some());
    assert!(PackedSectionBlockPos::from_raw(0x1000).is_none());
}

#[test]
fn packed_section_block_pos_rejects_invalid_local_coordinates() {
    assert!(PackedSectionBlockPos::from_local_xyz(15, 15, 15).is_some());
    assert!(PackedSectionBlockPos::from_local_xyz(16, 0, 0).is_none());
    assert!(PackedSectionBlockPos::from_local_xyz(0, 16, 0).is_none());
    assert!(PackedSectionBlockPos::from_local_xyz(0, 0, 16).is_none());
}

#[test]
fn packed_section_block_pos_converts_to_absolute_block_pos() {
    let section = SectionPos::new(2, -4, -3);
    let Some(packed) = PackedSectionBlockPos::from_local_xyz(1, 15, 2) else {
        panic!("valid local packed section block position was rejected");
    };

    assert_eq!(packed.to_block_pos(section), BlockPos::new(33, -49, -46));
    assert_eq!(
        section.relative_to_block_pos(packed),
        BlockPos::new(33, -49, -46)
    );
}

#[test]
fn packed_position_newtypes_roundtrip() {
    let chunk = ChunkPos::new(-12, 34);
    assert_eq!(PackedChunkPos::from(chunk).to_chunk_pos(), chunk);

    let block = BlockPos::new(-1024, 64, 2048);
    assert_eq!(PackedBlockPos::from(block).to_block_pos(), block);

    let section = SectionPos::new(-8, -4, 12);
    assert_eq!(PackedSectionPos::from(section).to_section_pos(), section);
}
