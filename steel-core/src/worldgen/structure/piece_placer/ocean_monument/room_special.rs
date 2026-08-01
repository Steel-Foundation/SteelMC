use super::{
    ScatteredFeaturePlacer, base_black, base_gray, base_light, generate_water_box, lamp,
    spawn_elder,
};

pub(super) fn place_wing_room(placer: &mut ScatteredFeaturePlacer<'_, '_>, main_design: i32) {
    if main_design == 0 {
        for i in 0..4 {
            placer.generate_box(
                10 - i,
                3 - i,
                20 - i,
                12 + i,
                3 - i,
                20,
                base_light(),
                base_light(),
                false,
            );
        }
        placer.generate_box(7, 0, 6, 15, 0, 16, base_light(), base_light(), false);
        placer.generate_box(6, 0, 6, 6, 3, 20, base_light(), base_light(), false);
        placer.generate_box(16, 0, 6, 16, 3, 20, base_light(), base_light(), false);
        placer.generate_box(7, 1, 7, 7, 1, 20, base_light(), base_light(), false);
        placer.generate_box(15, 1, 7, 15, 1, 20, base_light(), base_light(), false);
        placer.generate_box(7, 1, 6, 9, 3, 6, base_light(), base_light(), false);
        placer.generate_box(13, 1, 6, 15, 3, 6, base_light(), base_light(), false);
        placer.generate_box(8, 1, 7, 9, 1, 7, base_light(), base_light(), false);
        placer.generate_box(13, 1, 7, 14, 1, 7, base_light(), base_light(), false);
        placer.generate_box(9, 0, 5, 13, 0, 5, base_light(), base_light(), false);
        placer.generate_box(10, 0, 7, 12, 0, 7, base_black(), base_black(), false);
        placer.generate_box(8, 0, 10, 8, 0, 12, base_black(), base_black(), false);
        placer.generate_box(14, 0, 10, 14, 0, 12, base_black(), base_black(), false);
        for z in (7..=18).rev().step_by(3) {
            placer.place_block(lamp(), 6, 3, z);
            placer.place_block(lamp(), 16, 3, z);
        }
        placer.place_block(lamp(), 10, 0, 10);
        placer.place_block(lamp(), 12, 0, 10);
        placer.place_block(lamp(), 10, 0, 12);
        placer.place_block(lamp(), 12, 0, 12);
        placer.place_block(lamp(), 8, 3, 6);
        placer.place_block(lamp(), 14, 3, 6);
        for (x, z) in [(4, 4), (18, 4), (4, 18), (18, 18)] {
            placer.place_block(base_light(), x, 2, z);
            placer.place_block(lamp(), x, 1, z);
            placer.place_block(base_light(), x, 0, z);
        }
        placer.place_block(base_light(), 9, 7, 20);
        placer.place_block(base_light(), 13, 7, 20);
        placer.generate_box(6, 0, 21, 7, 4, 21, base_light(), base_light(), false);
        placer.generate_box(15, 0, 21, 16, 4, 21, base_light(), base_light(), false);
        spawn_elder(placer, 11, 2, 16);
    } else if main_design == 1 {
        placer.generate_box(9, 3, 18, 13, 3, 20, base_light(), base_light(), false);
        placer.generate_box(9, 0, 18, 9, 2, 18, base_light(), base_light(), false);
        placer.generate_box(13, 0, 18, 13, 2, 18, base_light(), base_light(), false);
        for x in [9, 13] {
            placer.place_block(base_light(), x, 6, 20);
            placer.place_block(lamp(), x, 5, 20);
            placer.place_block(base_light(), x, 4, 20);
        }
        placer.generate_box(7, 3, 7, 15, 3, 14, base_light(), base_light(), false);
        for x in [10, 12] {
            placer.generate_box(x, 0, 10, x, 6, 10, base_light(), base_light(), false);
            placer.generate_box(x, 0, 12, x, 6, 12, base_light(), base_light(), false);
            placer.place_block(lamp(), x, 0, 10);
            placer.place_block(lamp(), x, 0, 12);
            placer.place_block(lamp(), x, 4, 10);
            placer.place_block(lamp(), x, 4, 12);
        }
        for x in [8, 14] {
            placer.generate_box(x, 0, 7, x, 2, 7, base_light(), base_light(), false);
            placer.generate_box(x, 0, 14, x, 2, 14, base_light(), base_light(), false);
        }
        placer.generate_box(8, 3, 8, 8, 3, 13, base_black(), base_black(), false);
        placer.generate_box(14, 3, 8, 14, 3, 13, base_black(), base_black(), false);
        spawn_elder(placer, 11, 5, 13);
    }
}

pub(super) fn place_penthouse(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    placer.generate_box(2, -1, 2, 11, -1, 11, base_light(), base_light(), false);
    placer.generate_box(0, -1, 0, 1, -1, 11, base_gray(), base_gray(), false);
    placer.generate_box(12, -1, 0, 13, -1, 11, base_gray(), base_gray(), false);
    placer.generate_box(2, -1, 0, 11, -1, 1, base_gray(), base_gray(), false);
    placer.generate_box(2, -1, 12, 11, -1, 13, base_gray(), base_gray(), false);
    placer.generate_box(0, 0, 0, 0, 0, 13, base_light(), base_light(), false);
    placer.generate_box(13, 0, 0, 13, 0, 13, base_light(), base_light(), false);
    placer.generate_box(1, 0, 0, 12, 0, 0, base_light(), base_light(), false);
    placer.generate_box(1, 0, 13, 12, 0, 13, base_light(), base_light(), false);
    for i in (2..=11).step_by(3) {
        placer.place_block(lamp(), 0, 0, i);
        placer.place_block(lamp(), 13, 0, i);
        placer.place_block(lamp(), i, 0, 0);
    }
    placer.generate_box(2, 0, 3, 4, 0, 9, base_light(), base_light(), false);
    placer.generate_box(9, 0, 3, 11, 0, 9, base_light(), base_light(), false);
    placer.generate_box(4, 0, 9, 9, 0, 11, base_light(), base_light(), false);
    placer.place_block(base_light(), 5, 0, 8);
    placer.place_block(base_light(), 8, 0, 8);
    placer.place_block(base_light(), 10, 0, 10);
    placer.place_block(base_light(), 3, 0, 10);
    placer.generate_box(3, 0, 3, 3, 0, 7, base_black(), base_black(), false);
    placer.generate_box(10, 0, 3, 10, 0, 7, base_black(), base_black(), false);
    placer.generate_box(6, 0, 10, 7, 0, 10, base_black(), base_black(), false);
    for x in [3, 10] {
        for z in (2..=8).step_by(3) {
            placer.generate_box(x, 0, z, x, 2, z, base_light(), base_light(), false);
        }
    }
    placer.generate_box(5, 0, 10, 5, 2, 10, base_light(), base_light(), false);
    placer.generate_box(8, 0, 10, 8, 2, 10, base_light(), base_light(), false);
    placer.generate_box(6, -1, 7, 7, -1, 8, base_black(), base_black(), false);
    generate_water_box(placer, 6, -1, 3, 7, -1, 4);
    spawn_elder(placer, 6, 1, 6);
}
