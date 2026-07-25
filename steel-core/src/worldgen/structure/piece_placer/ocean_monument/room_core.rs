use super::{
    Direction, OceanMonumentRoomData, ScatteredFeaturePlacer, base_black, base_gray, base_light,
    generate_box_on_fill_only, generate_water_box, lamp, open, vanilla_blocks,
};

pub(super) fn place_core_room(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    generate_box_on_fill_only(placer, 1, 8, 0, 14, 8, 14, base_gray());
    placer.generate_box(0, 7, 0, 0, 7, 15, base_light(), base_light(), false);
    placer.generate_box(15, 7, 0, 15, 7, 15, base_light(), base_light(), false);
    placer.generate_box(1, 7, 0, 15, 7, 0, base_light(), base_light(), false);
    placer.generate_box(1, 7, 15, 14, 7, 15, base_light(), base_light(), false);

    for y in 1..=6 {
        let block = if y == 2 || y == 6 {
            base_gray()
        } else {
            base_light()
        };
        for x in (0..=15).step_by(15) {
            placer.generate_box(x, y, 0, x, y, 1, block, block, false);
            placer.generate_box(x, y, 6, x, y, 9, block, block, false);
            placer.generate_box(x, y, 14, x, y, 15, block, block, false);
        }
        placer.generate_box(1, y, 0, 1, y, 0, block, block, false);
        placer.generate_box(6, y, 0, 9, y, 0, block, block, false);
        placer.generate_box(14, y, 0, 14, y, 0, block, block, false);
        placer.generate_box(1, y, 15, 14, y, 15, block, block, false);
    }

    placer.generate_box(6, 3, 6, 9, 6, 9, base_black(), base_black(), false);
    placer.generate_box(
        7,
        4,
        7,
        8,
        5,
        8,
        vanilla_blocks::GOLD_BLOCK.default_state(),
        vanilla_blocks::GOLD_BLOCK.default_state(),
        false,
    );

    for y in (3..=6).step_by(3) {
        for x in (6..=9).step_by(3) {
            placer.place_block(lamp(), x, y, 6);
            placer.place_block(lamp(), x, y, 9);
        }
    }

    placer.generate_box(5, 1, 6, 5, 2, 6, base_light(), base_light(), false);
    placer.generate_box(5, 1, 9, 5, 2, 9, base_light(), base_light(), false);
    placer.generate_box(10, 1, 6, 10, 2, 6, base_light(), base_light(), false);
    placer.generate_box(10, 1, 9, 10, 2, 9, base_light(), base_light(), false);
    placer.generate_box(6, 1, 5, 6, 2, 5, base_light(), base_light(), false);
    placer.generate_box(9, 1, 5, 9, 2, 5, base_light(), base_light(), false);
    placer.generate_box(6, 1, 10, 6, 2, 10, base_light(), base_light(), false);
    placer.generate_box(9, 1, 10, 9, 2, 10, base_light(), base_light(), false);
    placer.generate_box(5, 2, 5, 5, 6, 5, base_light(), base_light(), false);
    placer.generate_box(5, 2, 10, 5, 6, 10, base_light(), base_light(), false);
    placer.generate_box(10, 2, 5, 10, 6, 5, base_light(), base_light(), false);
    placer.generate_box(10, 2, 10, 10, 6, 10, base_light(), base_light(), false);
    placer.generate_box(5, 7, 1, 5, 7, 6, base_light(), base_light(), false);
    placer.generate_box(10, 7, 1, 10, 7, 6, base_light(), base_light(), false);
    placer.generate_box(5, 7, 9, 5, 7, 14, base_light(), base_light(), false);
    placer.generate_box(10, 7, 9, 10, 7, 14, base_light(), base_light(), false);
    placer.generate_box(1, 7, 5, 6, 7, 5, base_light(), base_light(), false);
    placer.generate_box(1, 7, 10, 6, 7, 10, base_light(), base_light(), false);
    placer.generate_box(9, 7, 5, 14, 7, 5, base_light(), base_light(), false);
    placer.generate_box(9, 7, 10, 14, 7, 10, base_light(), base_light(), false);
    placer.generate_box(2, 1, 2, 2, 1, 3, base_light(), base_light(), false);
    placer.generate_box(3, 1, 2, 3, 1, 2, base_light(), base_light(), false);
    placer.generate_box(13, 1, 2, 13, 1, 3, base_light(), base_light(), false);
    placer.generate_box(12, 1, 2, 12, 1, 2, base_light(), base_light(), false);
    placer.generate_box(2, 1, 12, 2, 1, 13, base_light(), base_light(), false);
    placer.generate_box(3, 1, 13, 3, 1, 13, base_light(), base_light(), false);
    placer.generate_box(13, 1, 12, 13, 1, 13, base_light(), base_light(), false);
    placer.generate_box(12, 1, 13, 12, 1, 13, base_light(), base_light(), false);
}

pub(super) fn place_entry_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
) {
    placer.generate_box(0, 3, 0, 2, 3, 7, base_light(), base_light(), false);
    placer.generate_box(5, 3, 0, 7, 3, 7, base_light(), base_light(), false);
    placer.generate_box(0, 2, 0, 1, 2, 7, base_light(), base_light(), false);
    placer.generate_box(6, 2, 0, 7, 2, 7, base_light(), base_light(), false);
    placer.generate_box(0, 1, 0, 0, 1, 7, base_light(), base_light(), false);
    placer.generate_box(7, 1, 0, 7, 1, 7, base_light(), base_light(), false);
    placer.generate_box(0, 1, 7, 7, 3, 7, base_light(), base_light(), false);
    placer.generate_box(1, 1, 0, 2, 3, 0, base_light(), base_light(), false);
    placer.generate_box(5, 1, 0, 6, 3, 0, base_light(), base_light(), false);
    if open(room, Direction::North) {
        generate_water_box(placer, 3, 1, 7, 4, 2, 7);
    }
    if open(room, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 1, 2, 4);
    }
    if open(room, Direction::East) {
        generate_water_box(placer, 6, 1, 3, 7, 2, 4);
    }
}
