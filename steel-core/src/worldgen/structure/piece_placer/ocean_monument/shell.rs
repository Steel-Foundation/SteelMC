use super::{
    ScatteredFeaturePlacer, base_black, base_gray, base_light, dot_deco, generate_water_box, lamp,
};

pub(super) fn place_monument_building_shell(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    let water_height = placer.sea_level().max(64) - placer.world_pos(0, 0, 0).y();
    generate_water_box(placer, 0, 0, 0, 58, water_height, 58);
    generate_wing(placer, false, 0);
    generate_wing(placer, true, 33);
    generate_entrance_archs(placer);
    generate_entrance_wall(placer);
    generate_roof_piece(placer);
    generate_lower_wall(placer);
    generate_middle_wall(placer);
    generate_upper_wall(placer);

    for pillar_x in 0..7 {
        let mut pillar_z = 0;
        while pillar_z < 7 {
            if pillar_z == 0 && pillar_x == 3 {
                pillar_z = 6;
            }

            let bx = pillar_x * 9;
            let bz = pillar_z * 9;
            for w in 0..4 {
                for d in 0..4 {
                    placer.place_block(base_light(), bx + w, 0, bz + d);
                    placer.fill_column_down(base_light(), bx + w, -1, bz + d);
                }
            }

            if pillar_x != 0 && pillar_x != 6 {
                pillar_z += 6;
            } else {
                pillar_z += 1;
            }
        }
    }

    for i in 0..5 {
        generate_water_box(placer, -1 - i, i * 2, -1 - i, -1 - i, 23, 58 + i);
        generate_water_box(placer, 58 + i, i * 2, -1 - i, 58 + i, 23, 58 + i);
        generate_water_box(placer, -i, i * 2, -1 - i, 57 + i, 23, -1 - i);
        generate_water_box(placer, -i, i * 2, 58 + i, 57 + i, 23, 58 + i);
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "direct port of vanilla MonumentBuilding.generateWing"
)]
fn generate_wing(placer: &mut ScatteredFeaturePlacer<'_, '_>, is_flipped: bool, xoff: i32) {
    if !placer.chunk_intersects(xoff, 0, xoff + 23, 20) {
        return;
    }

    placer.generate_box(
        xoff,
        0,
        0,
        xoff + 24,
        0,
        20,
        base_gray(),
        base_gray(),
        false,
    );
    generate_water_box(placer, xoff, 1, 0, xoff + 24, 10, 20);

    for i in 0..4 {
        placer.generate_box(
            xoff + i,
            i + 1,
            i,
            xoff + i,
            i + 1,
            20,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + i + 7,
            i + 5,
            i + 7,
            xoff + i + 7,
            i + 5,
            20,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 17 - i,
            i + 5,
            i + 7,
            xoff + 17 - i,
            i + 5,
            20,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 24 - i,
            i + 1,
            i,
            xoff + 24 - i,
            i + 1,
            20,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + i + 1,
            i + 1,
            i,
            xoff + 23 - i,
            i + 1,
            i,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + i + 8,
            i + 5,
            i + 7,
            xoff + 16 - i,
            i + 5,
            i + 7,
            base_light(),
            base_light(),
            false,
        );
    }

    placer.generate_box(
        xoff + 4,
        4,
        4,
        xoff + 6,
        4,
        20,
        base_gray(),
        base_gray(),
        false,
    );
    placer.generate_box(
        xoff + 7,
        4,
        4,
        xoff + 17,
        4,
        6,
        base_gray(),
        base_gray(),
        false,
    );
    placer.generate_box(
        xoff + 18,
        4,
        4,
        xoff + 20,
        4,
        20,
        base_gray(),
        base_gray(),
        false,
    );
    placer.generate_box(
        xoff + 11,
        8,
        11,
        xoff + 13,
        8,
        20,
        base_gray(),
        base_gray(),
        false,
    );
    placer.place_block(dot_deco(), xoff + 12, 9, 12);
    placer.place_block(dot_deco(), xoff + 12, 9, 15);
    placer.place_block(dot_deco(), xoff + 12, 9, 18);

    let left_pos = xoff + if is_flipped { 19 } else { 5 };
    let right_pos = xoff + if is_flipped { 5 } else { 19 };
    for z in (5..=20).rev().step_by(3) {
        placer.place_block(dot_deco(), left_pos, 5, z);
    }
    for z in (7..=19).rev().step_by(3) {
        placer.place_block(dot_deco(), right_pos, 5, z);
    }
    for i in 0..4 {
        let pos = if is_flipped {
            xoff + 24 - (17 - i * 3)
        } else {
            xoff + 17 - i * 3
        };
        placer.place_block(dot_deco(), pos, 5, 5);
    }

    placer.place_block(dot_deco(), right_pos, 5, 5);
    placer.generate_box(
        xoff + 11,
        1,
        12,
        xoff + 13,
        7,
        12,
        base_gray(),
        base_gray(),
        false,
    );
    placer.generate_box(
        xoff + 12,
        1,
        11,
        xoff + 12,
        7,
        13,
        base_gray(),
        base_gray(),
        false,
    );
}

fn generate_entrance_archs(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if !placer.chunk_intersects(22, 5, 35, 17) {
        return;
    }

    generate_water_box(placer, 25, 0, 0, 32, 8, 20);
    for i in 0..4 {
        let z = 5 + i * 4;
        placer.generate_box(24, 2, z, 24, 4, z, base_light(), base_light(), false);
        placer.generate_box(22, 4, z, 23, 4, z, base_light(), base_light(), false);
        placer.place_block(base_light(), 25, 5, z);
        placer.place_block(base_light(), 26, 6, z);
        placer.place_block(lamp(), 26, 5, z);
        placer.generate_box(33, 2, z, 33, 4, z, base_light(), base_light(), false);
        placer.generate_box(34, 4, z, 35, 4, z, base_light(), base_light(), false);
        placer.place_block(base_light(), 32, 5, z);
        placer.place_block(base_light(), 31, 6, z);
        placer.place_block(lamp(), 31, 5, z);
        placer.generate_box(27, 6, z, 30, 6, z, base_gray(), base_gray(), false);
    }
}

fn generate_entrance_wall(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if !placer.chunk_intersects(15, 20, 42, 21) {
        return;
    }

    placer.generate_box(15, 0, 21, 42, 0, 21, base_gray(), base_gray(), false);
    generate_water_box(placer, 26, 1, 21, 31, 3, 21);
    placer.generate_box(21, 12, 21, 36, 12, 21, base_gray(), base_gray(), false);
    placer.generate_box(17, 11, 21, 40, 11, 21, base_gray(), base_gray(), false);
    placer.generate_box(16, 10, 21, 41, 10, 21, base_gray(), base_gray(), false);
    placer.generate_box(15, 7, 21, 42, 9, 21, base_gray(), base_gray(), false);
    placer.generate_box(16, 6, 21, 41, 6, 21, base_gray(), base_gray(), false);
    placer.generate_box(17, 5, 21, 40, 5, 21, base_gray(), base_gray(), false);
    placer.generate_box(21, 4, 21, 36, 4, 21, base_gray(), base_gray(), false);
    placer.generate_box(22, 3, 21, 26, 3, 21, base_gray(), base_gray(), false);
    placer.generate_box(31, 3, 21, 35, 3, 21, base_gray(), base_gray(), false);
    placer.generate_box(23, 2, 21, 25, 2, 21, base_gray(), base_gray(), false);
    placer.generate_box(32, 2, 21, 34, 2, 21, base_gray(), base_gray(), false);
    placer.generate_box(28, 4, 20, 29, 4, 21, base_light(), base_light(), false);
    placer.place_block(base_light(), 27, 3, 21);
    placer.place_block(base_light(), 30, 3, 21);
    placer.place_block(base_light(), 26, 2, 21);
    placer.place_block(base_light(), 31, 2, 21);
    placer.place_block(base_light(), 25, 1, 21);
    placer.place_block(base_light(), 32, 1, 21);

    for i in 0..7 {
        placer.place_block(base_black(), 28 - i, 6 + i, 21);
        placer.place_block(base_black(), 29 + i, 6 + i, 21);
    }
    for i in 0..4 {
        placer.place_block(base_black(), 28 - i, 9 + i, 21);
        placer.place_block(base_black(), 29 + i, 9 + i, 21);
    }
    placer.place_block(base_black(), 28, 12, 21);
    placer.place_block(base_black(), 29, 12, 21);
    for i in 0..3 {
        placer.place_block(base_black(), 22 - i * 2, 8, 21);
        placer.place_block(base_black(), 22 - i * 2, 9, 21);
        placer.place_block(base_black(), 35 + i * 2, 8, 21);
        placer.place_block(base_black(), 35 + i * 2, 9, 21);
    }

    generate_water_box(placer, 15, 13, 21, 42, 15, 21);
    generate_water_box(placer, 15, 1, 21, 15, 6, 21);
    generate_water_box(placer, 16, 1, 21, 16, 5, 21);
    generate_water_box(placer, 17, 1, 21, 20, 4, 21);
    generate_water_box(placer, 21, 1, 21, 21, 3, 21);
    generate_water_box(placer, 22, 1, 21, 22, 2, 21);
    generate_water_box(placer, 23, 1, 21, 24, 1, 21);
    generate_water_box(placer, 42, 1, 21, 42, 6, 21);
    generate_water_box(placer, 41, 1, 21, 41, 5, 21);
    generate_water_box(placer, 37, 1, 21, 40, 4, 21);
    generate_water_box(placer, 36, 1, 21, 36, 3, 21);
    generate_water_box(placer, 33, 1, 21, 34, 1, 21);
    generate_water_box(placer, 35, 1, 21, 35, 2, 21);
}

fn generate_roof_piece(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if !placer.chunk_intersects(21, 21, 36, 36) {
        return;
    }

    placer.generate_box(21, 0, 22, 36, 0, 36, base_gray(), base_gray(), false);
    generate_water_box(placer, 21, 1, 22, 36, 23, 36);
    for i in 0..4 {
        placer.generate_box(
            21 + i,
            13 + i,
            21 + i,
            36 - i,
            13 + i,
            21 + i,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            21 + i,
            13 + i,
            36 - i,
            36 - i,
            13 + i,
            36 - i,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            21 + i,
            13 + i,
            22 + i,
            21 + i,
            13 + i,
            35 - i,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            36 - i,
            13 + i,
            22 + i,
            36 - i,
            13 + i,
            35 - i,
            base_light(),
            base_light(),
            false,
        );
    }

    placer.generate_box(25, 16, 25, 32, 16, 32, base_gray(), base_gray(), false);
    placer.generate_box(25, 17, 25, 25, 19, 25, base_light(), base_light(), false);
    placer.generate_box(32, 17, 25, 32, 19, 25, base_light(), base_light(), false);
    placer.generate_box(25, 17, 32, 25, 19, 32, base_light(), base_light(), false);
    placer.generate_box(32, 17, 32, 32, 19, 32, base_light(), base_light(), false);
    placer.place_block(base_light(), 26, 20, 26);
    placer.place_block(base_light(), 27, 21, 27);
    placer.place_block(lamp(), 27, 20, 27);
    placer.place_block(base_light(), 26, 20, 31);
    placer.place_block(base_light(), 27, 21, 30);
    placer.place_block(lamp(), 27, 20, 30);
    placer.place_block(base_light(), 31, 20, 31);
    placer.place_block(base_light(), 30, 21, 30);
    placer.place_block(lamp(), 30, 20, 30);
    placer.place_block(base_light(), 31, 20, 26);
    placer.place_block(base_light(), 30, 21, 27);
    placer.place_block(lamp(), 30, 20, 27);
    placer.generate_box(28, 21, 27, 29, 21, 27, base_gray(), base_gray(), false);
    placer.generate_box(27, 21, 28, 27, 21, 29, base_gray(), base_gray(), false);
    placer.generate_box(28, 21, 30, 29, 21, 30, base_gray(), base_gray(), false);
    placer.generate_box(30, 21, 28, 30, 21, 29, base_gray(), base_gray(), false);
}

fn generate_lower_wall(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if placer.chunk_intersects(0, 21, 6, 58) {
        placer.generate_box(0, 0, 21, 6, 0, 57, base_gray(), base_gray(), false);
        generate_water_box(placer, 0, 1, 21, 6, 7, 57);
        placer.generate_box(4, 4, 21, 6, 4, 53, base_gray(), base_gray(), false);
        for i in 0..4 {
            placer.generate_box(
                i,
                i + 1,
                21,
                i,
                i + 1,
                57 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (23..53).step_by(3) {
            placer.place_block(dot_deco(), 5, 5, z);
        }
        placer.place_block(dot_deco(), 5, 5, 52);
        for i in 0..4 {
            placer.generate_box(
                i,
                i + 1,
                21,
                i,
                i + 1,
                57 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        placer.generate_box(4, 1, 52, 6, 3, 52, base_gray(), base_gray(), false);
        placer.generate_box(5, 1, 51, 5, 3, 53, base_gray(), base_gray(), false);
    }

    if placer.chunk_intersects(51, 21, 58, 58) {
        placer.generate_box(51, 0, 21, 57, 0, 57, base_gray(), base_gray(), false);
        generate_water_box(placer, 51, 1, 21, 57, 7, 57);
        placer.generate_box(51, 4, 21, 53, 4, 53, base_gray(), base_gray(), false);
        for i in 0..4 {
            placer.generate_box(
                57 - i,
                i + 1,
                21,
                57 - i,
                i + 1,
                57 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (23..53).step_by(3) {
            placer.place_block(dot_deco(), 52, 5, z);
        }
        placer.place_block(dot_deco(), 52, 5, 52);
        placer.generate_box(51, 1, 52, 53, 3, 52, base_gray(), base_gray(), false);
        placer.generate_box(52, 1, 51, 52, 3, 53, base_gray(), base_gray(), false);
    }

    if placer.chunk_intersects(0, 51, 57, 57) {
        placer.generate_box(7, 0, 51, 50, 0, 57, base_gray(), base_gray(), false);
        generate_water_box(placer, 7, 1, 51, 50, 10, 57);
        for i in 0..4 {
            placer.generate_box(
                i + 1,
                i + 1,
                57 - i,
                56 - i,
                i + 1,
                57 - i,
                base_light(),
                base_light(),
                false,
            );
        }
    }
}

fn generate_middle_wall(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if placer.chunk_intersects(7, 21, 13, 50) {
        placer.generate_box(7, 0, 21, 13, 0, 50, base_gray(), base_gray(), false);
        generate_water_box(placer, 7, 1, 21, 13, 10, 50);
        placer.generate_box(11, 8, 21, 13, 8, 53, base_gray(), base_gray(), false);
        for i in 0..4 {
            placer.generate_box(
                i + 7,
                i + 5,
                21,
                i + 7,
                i + 5,
                54,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (21..=45).step_by(3) {
            placer.place_block(dot_deco(), 12, 9, z);
        }
    }

    if placer.chunk_intersects(44, 21, 50, 54) {
        placer.generate_box(44, 0, 21, 50, 0, 50, base_gray(), base_gray(), false);
        generate_water_box(placer, 44, 1, 21, 50, 10, 50);
        placer.generate_box(44, 8, 21, 46, 8, 53, base_gray(), base_gray(), false);
        for i in 0..4 {
            placer.generate_box(
                50 - i,
                i + 5,
                21,
                50 - i,
                i + 5,
                54,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (21..=45).step_by(3) {
            placer.place_block(dot_deco(), 45, 9, z);
        }
    }

    if placer.chunk_intersects(8, 44, 49, 54) {
        placer.generate_box(14, 0, 44, 43, 0, 50, base_gray(), base_gray(), false);
        generate_water_box(placer, 14, 1, 44, 43, 10, 50);
        for x in (12..=45).step_by(3) {
            placer.place_block(dot_deco(), x, 9, 45);
            placer.place_block(dot_deco(), x, 9, 52);
            if matches!(x, 12 | 18 | 24 | 33 | 39 | 45) {
                placer.place_block(dot_deco(), x, 9, 47);
                placer.place_block(dot_deco(), x, 9, 50);
                placer.place_block(dot_deco(), x, 10, 45);
                placer.place_block(dot_deco(), x, 10, 46);
                placer.place_block(dot_deco(), x, 10, 51);
                placer.place_block(dot_deco(), x, 10, 52);
                placer.place_block(dot_deco(), x, 11, 47);
                placer.place_block(dot_deco(), x, 11, 50);
                placer.place_block(dot_deco(), x, 12, 48);
                placer.place_block(dot_deco(), x, 12, 49);
            }
        }
        for i in 0..3 {
            placer.generate_box(
                8 + i,
                5 + i,
                54,
                49 - i,
                5 + i,
                54,
                base_gray(),
                base_gray(),
                false,
            );
        }
        placer.generate_box(11, 8, 54, 46, 8, 54, base_light(), base_light(), false);
        placer.generate_box(14, 8, 44, 43, 8, 53, base_gray(), base_gray(), false);
    }
}

fn generate_upper_wall(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
    if placer.chunk_intersects(14, 21, 20, 43) {
        placer.generate_box(14, 0, 21, 20, 0, 43, base_gray(), base_gray(), false);
        generate_water_box(placer, 14, 1, 22, 20, 14, 43);
        placer.generate_box(18, 12, 22, 20, 12, 39, base_gray(), base_gray(), false);
        placer.generate_box(18, 12, 21, 20, 12, 21, base_light(), base_light(), false);
        for i in 0..4 {
            placer.generate_box(
                i + 14,
                i + 9,
                21,
                i + 14,
                i + 9,
                43 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (23..=39).step_by(3) {
            placer.place_block(dot_deco(), 19, 13, z);
        }
    }

    if placer.chunk_intersects(37, 21, 43, 43) {
        placer.generate_box(37, 0, 21, 43, 0, 43, base_gray(), base_gray(), false);
        generate_water_box(placer, 37, 1, 22, 43, 14, 43);
        placer.generate_box(37, 12, 22, 39, 12, 39, base_gray(), base_gray(), false);
        placer.generate_box(37, 12, 21, 39, 12, 21, base_light(), base_light(), false);
        for i in 0..4 {
            placer.generate_box(
                43 - i,
                i + 9,
                21,
                43 - i,
                i + 9,
                43 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        for z in (23..=39).step_by(3) {
            placer.place_block(dot_deco(), 38, 13, z);
        }
    }

    if placer.chunk_intersects(15, 37, 42, 43) {
        placer.generate_box(21, 0, 37, 36, 0, 43, base_gray(), base_gray(), false);
        generate_water_box(placer, 21, 1, 37, 36, 14, 43);
        placer.generate_box(21, 12, 37, 36, 12, 39, base_gray(), base_gray(), false);
        for i in 0..4 {
            placer.generate_box(
                15 + i,
                i + 9,
                43 - i,
                42 - i,
                i + 9,
                43 - i,
                base_light(),
                base_light(),
                false,
            );
        }
        for x in (21..=36).step_by(3) {
            placer.place_block(dot_deco(), x, 13, 38);
        }
    }
}
