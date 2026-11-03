use super::*;
use super::{
    grid::{MansionGrid, SimpleGrid},
    template::{MansionTemplatePiece, Mirror},
};

#[test]
fn recursive_corridor_branches_toward_selected_next_direction() {
    let mut grid = SimpleGrid::new(8, 8, 5);
    grid.set_cell(2, 3, 5);
    let mut rng = LegacyRandom::from_seed(0);

    MansionGrid::recursive_corridor(&mut grid, &mut rng, 4, 3, Direction::West, 2);

    assert_eq!(grid.get(3, 2), 1);
}

#[test]
fn mansion_piece_uses_template_payload_with_marker_handling() {
    let piece = MansionTemplatePiece::new(
        "entrance",
        IVec3::new(10, 64, 20),
        Rotation::Clockwise90,
        Mirror::LeftRight,
    );
    let expected_bounding_box = piece.bounding_box();

    let piece = piece.into_structure_piece();

    assert_eq!(piece.piece_type, Identifier::new_static("minecraft", "wmp"));
    assert_eq!(piece.bounding_box, expected_bounding_box);
    assert_eq!(piece.gen_depth, 0);
    assert_eq!(piece.orientation, Some(Direction::North));

    let StructurePiecePayload::Template(data) = piece.payload else {
        panic!("woodland mansion piece should be template-backed");
    };
    assert_eq!(
        data.template_id,
        Identifier::vanilla_static("woodland_mansion/entrance")
    );
    assert_eq!(data.template_position, IVec3::new(10, 64, 20));
    assert_eq!(data.rotation, Rotation::Clockwise90);
    assert_eq!(data.mirror, StructureMirror::LeftRight);
    assert_eq!(data.rotation_pivot, IVec3::ZERO);
    assert_eq!(data.block_ignore, StructureBlockIgnore::StructureBlock);
    assert_eq!(data.late_block_ignore, StructureBlockIgnore::None);
    assert_eq!(data.processors, TemplateProcessorList::Empty);
    assert_eq!(data.liquid_settings, LiquidSettingsData::ApplyWaterlogging);
    assert_eq!(
        data.marker_handling,
        TemplateMarkerHandling::WoodlandMansion
    );
    assert_eq!(data.placement_adjustment, TemplatePlacementAdjustment::None);
    assert_eq!(data.placement_clip, TemplatePlacementClip::CenterChunk);
    assert_eq!(data.post_process, TemplatePostProcess::None);
}

#[test]
fn secret_1x1_room_draws_the_regular_name_first() {
    let mut rng = LegacyRandom::from_seed(7);
    let mut pieces = Vec::new();
    super::rooms::add_room_1x1(&mut pieces, IVec3::ZERO, Rotation::None, None, 0, &mut rng);

    let mut expected = LegacyRandom::from_seed(7);
    let _regular = expected.next_i32_bounded(5);
    let secret = expected.next_i32_bounded(4) + 1;
    assert_eq!(
        pieces[0].template_id(),
        Identifier::vanilla(format!("woodland_mansion/1x1_as{secret}"))
    );
    assert_eq!(rng.next_i64(), expected.next_i64());
}
