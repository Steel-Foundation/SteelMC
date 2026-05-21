use std::sync::Arc;

use glam::DVec3;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::{Registry, vanilla_blocks, vanilla_entities};
use steel_utils::random::Random;
use steel_utils::random::worldgen_random::WorldgenRandom;
use steel_utils::{BlockStateId, BoundingBox, Direction};

use crate::entity::{entities::RawEntity, next_entity_id};
use crate::world::structure::ocean_monument::{
    OceanMonumentChildPiece, OceanMonumentChildPieceKind, OceanMonumentPieceData,
    OceanMonumentRoomData,
};
use crate::worldgen::region::WorldGenRegion;

use super::StructurePiecePlacer;
use super::scattered_feature::ScatteredFeaturePlacer;

impl StructurePiecePlacer {
    pub(super) fn place_ocean_monument_piece(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounding_box: BoundingBox,
        orientation: Option<Direction>,
        data: &mut OceanMonumentPieceData,
        clip: BoundingBox,
        random: &mut WorldgenRandom,
    ) -> bool {
        {
            let mut building_box = bounding_box;
            let mut placer =
                ScatteredFeaturePlacer::new(region, registry, &mut building_box, orientation, clip);
            place_monument_building_shell(&mut placer);
        }

        for child in &data.child_pieces {
            if !child.bounding_box.intersects(&clip) {
                continue;
            }

            let mut child_box = child.bounding_box;
            let mut placer =
                ScatteredFeaturePlacer::new(region, registry, &mut child_box, orientation, clip);
            place_child_piece(&mut placer, child, random);
        }

        true
    }
}

fn place_child_piece(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    child: &OceanMonumentChildPiece,
    random: &mut WorldgenRandom,
) {
    match &child.kind {
        OceanMonumentChildPieceKind::EntryRoom { room } => place_entry_room(placer, *room),
        OceanMonumentChildPieceKind::CoreRoom => place_core_room(placer),
        OceanMonumentChildPieceKind::DoubleXRoom { west, east } => {
            place_double_x_room(placer, *west, *east);
        }
        OceanMonumentChildPieceKind::DoubleXYRoom {
            west,
            east,
            west_up,
            east_up,
        } => place_double_xy_room(placer, *west, *east, *west_up, *east_up),
        OceanMonumentChildPieceKind::DoubleYRoom { room, above } => {
            place_double_y_room(placer, *room, *above);
        }
        OceanMonumentChildPieceKind::DoubleYZRoom {
            south,
            north,
            south_up,
            north_up,
        } => place_double_yz_room(placer, *south, *north, *south_up, *north_up),
        OceanMonumentChildPieceKind::DoubleZRoom { south, north } => {
            place_double_z_room(placer, *south, *north);
        }
        OceanMonumentChildPieceKind::SimpleRoom { room, main_design } => {
            place_simple_room(placer, random, *room, *main_design);
        }
        OceanMonumentChildPieceKind::SimpleTopRoom { room } => {
            place_simple_top_room(placer, random, *room);
        }
        OceanMonumentChildPieceKind::WingRoom { main_design } => {
            place_wing_room(placer, *main_design);
        }
        OceanMonumentChildPieceKind::Penthouse => place_penthouse(placer),
    }
}

fn place_monument_building_shell(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
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

fn place_core_room(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
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

fn place_entry_room(placer: &mut ScatteredFeaturePlacer<'_, '_>, room: OceanMonumentRoomData) {
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

fn place_double_x_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    west: OceanMonumentRoomData,
    east: OceanMonumentRoomData,
) {
    if west.index / 25 > 0 {
        generate_default_floor(placer, 8, 0, open(east, Direction::Down));
        generate_default_floor(placer, 0, 0, open(west, Direction::Down));
    }
    if !west.has_up_connection {
        generate_box_on_fill_only(placer, 1, 4, 1, 7, 4, 6, base_gray());
    }
    if !east.has_up_connection {
        generate_box_on_fill_only(placer, 8, 4, 1, 14, 4, 6, base_gray());
    }

    placer.generate_box(0, 3, 0, 0, 3, 7, base_light(), base_light(), false);
    placer.generate_box(15, 3, 0, 15, 3, 7, base_light(), base_light(), false);
    placer.generate_box(1, 3, 0, 15, 3, 0, base_light(), base_light(), false);
    placer.generate_box(1, 3, 7, 14, 3, 7, base_light(), base_light(), false);
    placer.generate_box(0, 2, 0, 0, 2, 7, base_gray(), base_gray(), false);
    placer.generate_box(15, 2, 0, 15, 2, 7, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 0, 15, 2, 0, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 7, 14, 2, 7, base_gray(), base_gray(), false);
    placer.generate_box(0, 1, 0, 0, 1, 7, base_light(), base_light(), false);
    placer.generate_box(15, 1, 0, 15, 1, 7, base_light(), base_light(), false);
    placer.generate_box(1, 1, 0, 15, 1, 0, base_light(), base_light(), false);
    placer.generate_box(1, 1, 7, 14, 1, 7, base_light(), base_light(), false);
    placer.generate_box(5, 1, 0, 10, 1, 4, base_light(), base_light(), false);
    placer.generate_box(6, 2, 0, 9, 2, 3, base_gray(), base_gray(), false);
    placer.generate_box(5, 3, 0, 10, 3, 4, base_light(), base_light(), false);
    placer.place_block(lamp(), 6, 2, 3);
    placer.place_block(lamp(), 9, 2, 3);

    if open(west, Direction::South) {
        generate_water_box(placer, 3, 1, 0, 4, 2, 0);
    }
    if open(west, Direction::North) {
        generate_water_box(placer, 3, 1, 7, 4, 2, 7);
    }
    if open(west, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 0, 2, 4);
    }
    if open(east, Direction::South) {
        generate_water_box(placer, 11, 1, 0, 12, 2, 0);
    }
    if open(east, Direction::North) {
        generate_water_box(placer, 11, 1, 7, 12, 2, 7);
    }
    if open(east, Direction::East) {
        generate_water_box(placer, 15, 1, 3, 15, 2, 4);
    }
}

fn place_double_xy_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    west: OceanMonumentRoomData,
    east: OceanMonumentRoomData,
    west_up: OceanMonumentRoomData,
    east_up: OceanMonumentRoomData,
) {
    if west.index / 25 > 0 {
        generate_default_floor(placer, 8, 0, open(east, Direction::Down));
        generate_default_floor(placer, 0, 0, open(west, Direction::Down));
    }
    if !west_up.has_up_connection {
        generate_box_on_fill_only(placer, 1, 8, 1, 7, 8, 6, base_gray());
    }
    if !east_up.has_up_connection {
        generate_box_on_fill_only(placer, 8, 8, 1, 14, 8, 6, base_gray());
    }

    for y in 1..=7 {
        let block = if y == 2 || y == 6 {
            base_gray()
        } else {
            base_light()
        };
        placer.generate_box(0, y, 0, 0, y, 7, block, block, false);
        placer.generate_box(15, y, 0, 15, y, 7, block, block, false);
        placer.generate_box(1, y, 0, 15, y, 0, block, block, false);
        placer.generate_box(1, y, 7, 14, y, 7, block, block, false);
    }

    placer.generate_box(2, 1, 3, 2, 7, 4, base_light(), base_light(), false);
    placer.generate_box(3, 1, 2, 4, 7, 2, base_light(), base_light(), false);
    placer.generate_box(3, 1, 5, 4, 7, 5, base_light(), base_light(), false);
    placer.generate_box(13, 1, 3, 13, 7, 4, base_light(), base_light(), false);
    placer.generate_box(11, 1, 2, 12, 7, 2, base_light(), base_light(), false);
    placer.generate_box(11, 1, 5, 12, 7, 5, base_light(), base_light(), false);
    placer.generate_box(5, 1, 3, 5, 3, 4, base_light(), base_light(), false);
    placer.generate_box(10, 1, 3, 10, 3, 4, base_light(), base_light(), false);
    placer.generate_box(5, 7, 2, 10, 7, 5, base_light(), base_light(), false);
    placer.generate_box(5, 5, 2, 5, 7, 2, base_light(), base_light(), false);
    placer.generate_box(10, 5, 2, 10, 7, 2, base_light(), base_light(), false);
    placer.generate_box(5, 5, 5, 5, 7, 5, base_light(), base_light(), false);
    placer.generate_box(10, 5, 5, 10, 7, 5, base_light(), base_light(), false);
    placer.place_block(base_light(), 6, 6, 2);
    placer.place_block(base_light(), 9, 6, 2);
    placer.place_block(base_light(), 6, 6, 5);
    placer.place_block(base_light(), 9, 6, 5);
    placer.generate_box(5, 4, 3, 6, 4, 4, base_light(), base_light(), false);
    placer.generate_box(9, 4, 3, 10, 4, 4, base_light(), base_light(), false);
    placer.place_block(lamp(), 5, 4, 2);
    placer.place_block(lamp(), 5, 4, 5);
    placer.place_block(lamp(), 10, 4, 2);
    placer.place_block(lamp(), 10, 4, 5);

    if open(west, Direction::South) {
        generate_water_box(placer, 3, 1, 0, 4, 2, 0);
    }
    if open(west, Direction::North) {
        generate_water_box(placer, 3, 1, 7, 4, 2, 7);
    }
    if open(west, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 0, 2, 4);
    }
    if open(east, Direction::South) {
        generate_water_box(placer, 11, 1, 0, 12, 2, 0);
    }
    if open(east, Direction::North) {
        generate_water_box(placer, 11, 1, 7, 12, 2, 7);
    }
    if open(east, Direction::East) {
        generate_water_box(placer, 15, 1, 3, 15, 2, 4);
    }
    if open(west_up, Direction::South) {
        generate_water_box(placer, 3, 5, 0, 4, 6, 0);
    }
    if open(west_up, Direction::North) {
        generate_water_box(placer, 3, 5, 7, 4, 6, 7);
    }
    if open(west_up, Direction::West) {
        generate_water_box(placer, 0, 5, 3, 0, 6, 4);
    }
    if open(east_up, Direction::South) {
        generate_water_box(placer, 11, 5, 0, 12, 6, 0);
    }
    if open(east_up, Direction::North) {
        generate_water_box(placer, 11, 5, 7, 12, 6, 7);
    }
    if open(east_up, Direction::East) {
        generate_water_box(placer, 15, 5, 3, 15, 6, 4);
    }
}

fn place_double_y_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
    above: OceanMonumentRoomData,
) {
    if room.index / 25 > 0 {
        generate_default_floor(placer, 0, 0, open(room, Direction::Down));
    }
    if !above.has_up_connection {
        generate_box_on_fill_only(placer, 1, 8, 1, 6, 8, 6, base_gray());
    }

    placer.generate_box(0, 4, 0, 0, 4, 7, base_light(), base_light(), false);
    placer.generate_box(7, 4, 0, 7, 4, 7, base_light(), base_light(), false);
    placer.generate_box(1, 4, 0, 6, 4, 0, base_light(), base_light(), false);
    placer.generate_box(1, 4, 7, 6, 4, 7, base_light(), base_light(), false);
    placer.generate_box(2, 4, 1, 2, 4, 2, base_light(), base_light(), false);
    placer.generate_box(1, 4, 2, 1, 4, 2, base_light(), base_light(), false);
    placer.generate_box(5, 4, 1, 5, 4, 2, base_light(), base_light(), false);
    placer.generate_box(6, 4, 2, 6, 4, 2, base_light(), base_light(), false);
    placer.generate_box(2, 4, 5, 2, 4, 6, base_light(), base_light(), false);
    placer.generate_box(1, 4, 5, 1, 4, 5, base_light(), base_light(), false);
    placer.generate_box(5, 4, 5, 5, 4, 6, base_light(), base_light(), false);
    placer.generate_box(6, 4, 5, 6, 4, 5, base_light(), base_light(), false);

    let rooms = [room, above];
    for (idx, definition) in rooms.into_iter().enumerate() {
        let y = 1 + (idx as i32) * 4;
        place_double_y_side_walls(placer, definition, y);
    }
}

fn place_double_y_side_walls(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
    y: i32,
) {
    if open(room, Direction::South) {
        placer.generate_box(2, y, 0, 2, y + 2, 0, base_light(), base_light(), false);
        placer.generate_box(5, y, 0, 5, y + 2, 0, base_light(), base_light(), false);
        placer.generate_box(3, y + 2, 0, 4, y + 2, 0, base_light(), base_light(), false);
    } else {
        placer.generate_box(0, y, 0, 7, y + 2, 0, base_light(), base_light(), false);
        placer.generate_box(0, y + 1, 0, 7, y + 1, 0, base_gray(), base_gray(), false);
    }

    if open(room, Direction::North) {
        placer.generate_box(2, y, 7, 2, y + 2, 7, base_light(), base_light(), false);
        placer.generate_box(5, y, 7, 5, y + 2, 7, base_light(), base_light(), false);
        placer.generate_box(3, y + 2, 7, 4, y + 2, 7, base_light(), base_light(), false);
    } else {
        placer.generate_box(0, y, 7, 7, y + 2, 7, base_light(), base_light(), false);
        placer.generate_box(0, y + 1, 7, 7, y + 1, 7, base_gray(), base_gray(), false);
    }

    if open(room, Direction::West) {
        placer.generate_box(0, y, 2, 0, y + 2, 2, base_light(), base_light(), false);
        placer.generate_box(0, y, 5, 0, y + 2, 5, base_light(), base_light(), false);
        placer.generate_box(0, y + 2, 3, 0, y + 2, 4, base_light(), base_light(), false);
    } else {
        placer.generate_box(0, y, 0, 0, y + 2, 7, base_light(), base_light(), false);
        placer.generate_box(0, y + 1, 0, 0, y + 1, 7, base_gray(), base_gray(), false);
    }

    if open(room, Direction::East) {
        placer.generate_box(7, y, 2, 7, y + 2, 2, base_light(), base_light(), false);
        placer.generate_box(7, y, 5, 7, y + 2, 5, base_light(), base_light(), false);
        placer.generate_box(7, y + 2, 3, 7, y + 2, 4, base_light(), base_light(), false);
    } else {
        placer.generate_box(7, y, 0, 7, y + 2, 7, base_light(), base_light(), false);
        placer.generate_box(7, y + 1, 0, 7, y + 1, 7, base_gray(), base_gray(), false);
    }
}

fn place_double_yz_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    south: OceanMonumentRoomData,
    north: OceanMonumentRoomData,
    south_up: OceanMonumentRoomData,
    north_up: OceanMonumentRoomData,
) {
    if south.index / 25 > 0 {
        generate_default_floor(placer, 0, 8, open(north, Direction::Down));
        generate_default_floor(placer, 0, 0, open(south, Direction::Down));
    }
    if !south_up.has_up_connection {
        generate_box_on_fill_only(placer, 1, 8, 1, 6, 8, 7, base_gray());
    }
    if !north_up.has_up_connection {
        generate_box_on_fill_only(placer, 1, 8, 8, 6, 8, 14, base_gray());
    }

    for y in 1..=7 {
        let block = if y == 2 || y == 6 {
            base_gray()
        } else {
            base_light()
        };
        placer.generate_box(0, y, 0, 0, y, 15, block, block, false);
        placer.generate_box(7, y, 0, 7, y, 15, block, block, false);
        placer.generate_box(1, y, 0, 6, y, 0, block, block, false);
        placer.generate_box(1, y, 15, 6, y, 15, block, block, false);
    }
    for y in 1..=7 {
        let block = if y == 2 || y == 6 {
            lamp()
        } else {
            base_black()
        };
        placer.generate_box(3, y, 7, 4, y, 8, block, block, false);
    }

    if open(south, Direction::South) {
        generate_water_box(placer, 3, 1, 0, 4, 2, 0);
    }
    if open(south, Direction::East) {
        generate_water_box(placer, 7, 1, 3, 7, 2, 4);
    }
    if open(south, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 0, 2, 4);
    }
    if open(north, Direction::North) {
        generate_water_box(placer, 3, 1, 15, 4, 2, 15);
    }
    if open(north, Direction::West) {
        generate_water_box(placer, 0, 1, 11, 0, 2, 12);
    }
    if open(north, Direction::East) {
        generate_water_box(placer, 7, 1, 11, 7, 2, 12);
    }
    if open(south_up, Direction::South) {
        generate_water_box(placer, 3, 5, 0, 4, 6, 0);
    }
    if open(south_up, Direction::East) {
        generate_water_box(placer, 7, 5, 3, 7, 6, 4);
        placer.generate_box(5, 4, 2, 6, 4, 5, base_light(), base_light(), false);
        placer.generate_box(6, 1, 2, 6, 3, 2, base_light(), base_light(), false);
        placer.generate_box(6, 1, 5, 6, 3, 5, base_light(), base_light(), false);
    }
    if open(south_up, Direction::West) {
        generate_water_box(placer, 0, 5, 3, 0, 6, 4);
        placer.generate_box(1, 4, 2, 2, 4, 5, base_light(), base_light(), false);
        placer.generate_box(1, 1, 2, 1, 3, 2, base_light(), base_light(), false);
        placer.generate_box(1, 1, 5, 1, 3, 5, base_light(), base_light(), false);
    }
    if open(north_up, Direction::North) {
        generate_water_box(placer, 3, 5, 15, 4, 6, 15);
    }
    if open(north_up, Direction::West) {
        generate_water_box(placer, 0, 5, 11, 0, 6, 12);
        placer.generate_box(1, 4, 10, 2, 4, 13, base_light(), base_light(), false);
        placer.generate_box(1, 1, 10, 1, 3, 10, base_light(), base_light(), false);
        placer.generate_box(1, 1, 13, 1, 3, 13, base_light(), base_light(), false);
    }
    if open(north_up, Direction::East) {
        generate_water_box(placer, 7, 5, 11, 7, 6, 12);
        placer.generate_box(5, 4, 10, 6, 4, 13, base_light(), base_light(), false);
        placer.generate_box(6, 1, 10, 6, 3, 10, base_light(), base_light(), false);
        placer.generate_box(6, 1, 13, 6, 3, 13, base_light(), base_light(), false);
    }
}

fn place_double_z_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    south: OceanMonumentRoomData,
    north: OceanMonumentRoomData,
) {
    if south.index / 25 > 0 {
        generate_default_floor(placer, 0, 8, open(north, Direction::Down));
        generate_default_floor(placer, 0, 0, open(south, Direction::Down));
    }
    if !south.has_up_connection {
        generate_box_on_fill_only(placer, 1, 4, 1, 6, 4, 7, base_gray());
    }
    if !north.has_up_connection {
        generate_box_on_fill_only(placer, 1, 4, 8, 6, 4, 14, base_gray());
    }

    placer.generate_box(0, 3, 0, 0, 3, 15, base_light(), base_light(), false);
    placer.generate_box(7, 3, 0, 7, 3, 15, base_light(), base_light(), false);
    placer.generate_box(1, 3, 0, 7, 3, 0, base_light(), base_light(), false);
    placer.generate_box(1, 3, 15, 6, 3, 15, base_light(), base_light(), false);
    placer.generate_box(0, 2, 0, 0, 2, 15, base_gray(), base_gray(), false);
    placer.generate_box(7, 2, 0, 7, 2, 15, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 0, 7, 2, 0, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 15, 6, 2, 15, base_gray(), base_gray(), false);
    placer.generate_box(0, 1, 0, 0, 1, 15, base_light(), base_light(), false);
    placer.generate_box(7, 1, 0, 7, 1, 15, base_light(), base_light(), false);
    placer.generate_box(1, 1, 0, 7, 1, 0, base_light(), base_light(), false);
    placer.generate_box(1, 1, 15, 6, 1, 15, base_light(), base_light(), false);
    placer.generate_box(1, 1, 1, 1, 1, 2, base_light(), base_light(), false);
    placer.generate_box(6, 1, 1, 6, 1, 2, base_light(), base_light(), false);
    placer.generate_box(1, 3, 1, 1, 3, 2, base_light(), base_light(), false);
    placer.generate_box(6, 3, 1, 6, 3, 2, base_light(), base_light(), false);
    placer.generate_box(1, 1, 13, 1, 1, 14, base_light(), base_light(), false);
    placer.generate_box(6, 1, 13, 6, 1, 14, base_light(), base_light(), false);
    placer.generate_box(1, 3, 13, 1, 3, 14, base_light(), base_light(), false);
    placer.generate_box(6, 3, 13, 6, 3, 14, base_light(), base_light(), false);
    placer.generate_box(2, 1, 6, 2, 3, 6, base_light(), base_light(), false);
    placer.generate_box(5, 1, 6, 5, 3, 6, base_light(), base_light(), false);
    placer.generate_box(2, 1, 9, 2, 3, 9, base_light(), base_light(), false);
    placer.generate_box(5, 1, 9, 5, 3, 9, base_light(), base_light(), false);
    placer.generate_box(3, 2, 6, 4, 2, 6, base_light(), base_light(), false);
    placer.generate_box(3, 2, 9, 4, 2, 9, base_light(), base_light(), false);
    placer.generate_box(2, 2, 7, 2, 2, 8, base_light(), base_light(), false);
    placer.generate_box(5, 2, 7, 5, 2, 8, base_light(), base_light(), false);
    placer.place_block(lamp(), 2, 2, 5);
    placer.place_block(lamp(), 5, 2, 5);
    placer.place_block(lamp(), 2, 2, 10);
    placer.place_block(lamp(), 5, 2, 10);
    placer.place_block(base_light(), 2, 3, 5);
    placer.place_block(base_light(), 5, 3, 5);
    placer.place_block(base_light(), 2, 3, 10);
    placer.place_block(base_light(), 5, 3, 10);

    if open(south, Direction::South) {
        generate_water_box(placer, 3, 1, 0, 4, 2, 0);
    }
    if open(south, Direction::East) {
        generate_water_box(placer, 7, 1, 3, 7, 2, 4);
    }
    if open(south, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 0, 2, 4);
    }
    if open(north, Direction::North) {
        generate_water_box(placer, 3, 1, 15, 4, 2, 15);
    }
    if open(north, Direction::West) {
        generate_water_box(placer, 0, 1, 11, 0, 2, 12);
    }
    if open(north, Direction::East) {
        generate_water_box(placer, 7, 1, 11, 7, 2, 12);
    }
}

fn place_simple_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    room: OceanMonumentRoomData,
    main_design: i32,
) {
    if room.index / 25 > 0 {
        generate_default_floor(placer, 0, 0, open(room, Direction::Down));
    }
    if !room.has_up_connection {
        generate_box_on_fill_only(placer, 1, 4, 1, 6, 4, 6, base_gray());
    }

    let center_pillar = main_design != 0
        && random.next_bool()
        && !open(room, Direction::Down)
        && !open(room, Direction::Up)
        && room.count_openings() > 1;
    if main_design == 0 {
        place_simple_room_design0(placer, room);
    } else if main_design == 1 {
        place_simple_room_design1(placer, room);
    } else if main_design == 2 {
        place_simple_room_design2(placer, room);
    }

    if center_pillar {
        placer.generate_box(3, 1, 3, 4, 1, 4, base_light(), base_light(), false);
        placer.generate_box(3, 2, 3, 4, 2, 4, base_gray(), base_gray(), false);
        placer.generate_box(3, 3, 3, 4, 3, 4, base_light(), base_light(), false);
    }
}

fn place_simple_room_design0(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
) {
    for (x0, z0) in [(0, 0), (5, 0), (0, 5), (5, 5)] {
        placer.generate_box(
            x0,
            1,
            z0,
            x0 + 2,
            1,
            z0 + 2,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            x0,
            3,
            z0,
            x0 + 2,
            3,
            z0 + 2,
            base_light(),
            base_light(),
            false,
        );
    }
    placer.generate_box(0, 2, 0, 0, 2, 2, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 0, 2, 2, 0, base_gray(), base_gray(), false);
    placer.place_block(lamp(), 1, 2, 1);
    placer.generate_box(7, 2, 0, 7, 2, 2, base_gray(), base_gray(), false);
    placer.generate_box(5, 2, 0, 6, 2, 0, base_gray(), base_gray(), false);
    placer.place_block(lamp(), 6, 2, 1);
    placer.generate_box(0, 2, 5, 0, 2, 7, base_gray(), base_gray(), false);
    placer.generate_box(1, 2, 7, 2, 2, 7, base_gray(), base_gray(), false);
    placer.place_block(lamp(), 1, 2, 6);
    placer.generate_box(7, 2, 5, 7, 2, 7, base_gray(), base_gray(), false);
    placer.generate_box(5, 2, 7, 6, 2, 7, base_gray(), base_gray(), false);
    placer.place_block(lamp(), 6, 2, 6);

    if open(room, Direction::South) {
        placer.generate_box(3, 3, 0, 4, 3, 0, base_light(), base_light(), false);
    } else {
        placer.generate_box(3, 3, 0, 4, 3, 1, base_light(), base_light(), false);
        placer.generate_box(3, 2, 0, 4, 2, 0, base_gray(), base_gray(), false);
        placer.generate_box(3, 1, 0, 4, 1, 1, base_light(), base_light(), false);
    }

    if open(room, Direction::North) {
        placer.generate_box(3, 3, 7, 4, 3, 7, base_light(), base_light(), false);
    } else {
        placer.generate_box(3, 3, 6, 4, 3, 7, base_light(), base_light(), false);
        placer.generate_box(3, 2, 7, 4, 2, 7, base_gray(), base_gray(), false);
        placer.generate_box(3, 1, 6, 4, 1, 7, base_light(), base_light(), false);
    }

    if open(room, Direction::West) {
        placer.generate_box(0, 3, 3, 0, 3, 4, base_light(), base_light(), false);
    } else {
        placer.generate_box(0, 3, 3, 1, 3, 4, base_light(), base_light(), false);
        placer.generate_box(0, 2, 3, 0, 2, 4, base_gray(), base_gray(), false);
        placer.generate_box(0, 1, 3, 1, 1, 4, base_light(), base_light(), false);
    }

    if open(room, Direction::East) {
        placer.generate_box(7, 3, 3, 7, 3, 4, base_light(), base_light(), false);
    } else {
        placer.generate_box(6, 3, 3, 7, 3, 4, base_light(), base_light(), false);
        placer.generate_box(7, 2, 3, 7, 2, 4, base_gray(), base_gray(), false);
        placer.generate_box(6, 1, 3, 7, 1, 4, base_light(), base_light(), false);
    }
}

fn place_simple_room_design1(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
) {
    for (x, z) in [(2, 2), (2, 5), (5, 5), (5, 2)] {
        placer.generate_box(x, 1, z, x, 3, z, base_light(), base_light(), false);
        placer.place_block(lamp(), x, 2, z);
    }
    placer.generate_box(0, 1, 0, 1, 3, 0, base_light(), base_light(), false);
    placer.generate_box(0, 1, 1, 0, 3, 1, base_light(), base_light(), false);
    placer.generate_box(0, 1, 7, 1, 3, 7, base_light(), base_light(), false);
    placer.generate_box(0, 1, 6, 0, 3, 6, base_light(), base_light(), false);
    placer.generate_box(6, 1, 7, 7, 3, 7, base_light(), base_light(), false);
    placer.generate_box(7, 1, 6, 7, 3, 6, base_light(), base_light(), false);
    placer.generate_box(6, 1, 0, 7, 3, 0, base_light(), base_light(), false);
    placer.generate_box(7, 1, 1, 7, 3, 1, base_light(), base_light(), false);
    placer.place_block(base_gray(), 1, 2, 0);
    placer.place_block(base_gray(), 0, 2, 1);
    placer.place_block(base_gray(), 1, 2, 7);
    placer.place_block(base_gray(), 0, 2, 6);
    placer.place_block(base_gray(), 6, 2, 7);
    placer.place_block(base_gray(), 7, 2, 6);
    placer.place_block(base_gray(), 6, 2, 0);
    placer.place_block(base_gray(), 7, 2, 1);

    if !open(room, Direction::South) {
        placer.generate_box(1, 3, 0, 6, 3, 0, base_light(), base_light(), false);
        placer.generate_box(1, 2, 0, 6, 2, 0, base_gray(), base_gray(), false);
        placer.generate_box(1, 1, 0, 6, 1, 0, base_light(), base_light(), false);
    }
    if !open(room, Direction::North) {
        placer.generate_box(1, 3, 7, 6, 3, 7, base_light(), base_light(), false);
        placer.generate_box(1, 2, 7, 6, 2, 7, base_gray(), base_gray(), false);
        placer.generate_box(1, 1, 7, 6, 1, 7, base_light(), base_light(), false);
    }
    if !open(room, Direction::West) {
        placer.generate_box(0, 3, 1, 0, 3, 6, base_light(), base_light(), false);
        placer.generate_box(0, 2, 1, 0, 2, 6, base_gray(), base_gray(), false);
        placer.generate_box(0, 1, 1, 0, 1, 6, base_light(), base_light(), false);
    }
    if !open(room, Direction::East) {
        placer.generate_box(7, 3, 1, 7, 3, 6, base_light(), base_light(), false);
        placer.generate_box(7, 2, 1, 7, 2, 6, base_gray(), base_gray(), false);
        placer.generate_box(7, 1, 1, 7, 1, 6, base_light(), base_light(), false);
    }
}

fn place_simple_room_design2(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    room: OceanMonumentRoomData,
) {
    placer.generate_box(0, 1, 0, 0, 1, 7, base_light(), base_light(), false);
    placer.generate_box(7, 1, 0, 7, 1, 7, base_light(), base_light(), false);
    placer.generate_box(1, 1, 0, 6, 1, 0, base_light(), base_light(), false);
    placer.generate_box(1, 1, 7, 6, 1, 7, base_light(), base_light(), false);
    placer.generate_box(0, 2, 0, 0, 2, 7, base_black(), base_black(), false);
    placer.generate_box(7, 2, 0, 7, 2, 7, base_black(), base_black(), false);
    placer.generate_box(1, 2, 0, 6, 2, 0, base_black(), base_black(), false);
    placer.generate_box(1, 2, 7, 6, 2, 7, base_black(), base_black(), false);
    placer.generate_box(0, 3, 0, 0, 3, 7, base_light(), base_light(), false);
    placer.generate_box(7, 3, 0, 7, 3, 7, base_light(), base_light(), false);
    placer.generate_box(1, 3, 0, 6, 3, 0, base_light(), base_light(), false);
    placer.generate_box(1, 3, 7, 6, 3, 7, base_light(), base_light(), false);
    placer.generate_box(0, 1, 3, 0, 2, 4, base_black(), base_black(), false);
    placer.generate_box(7, 1, 3, 7, 2, 4, base_black(), base_black(), false);
    placer.generate_box(3, 1, 0, 4, 2, 0, base_black(), base_black(), false);
    placer.generate_box(3, 1, 7, 4, 2, 7, base_black(), base_black(), false);

    if open(room, Direction::South) {
        generate_water_box(placer, 3, 1, 0, 4, 2, 0);
    }
    if open(room, Direction::North) {
        generate_water_box(placer, 3, 1, 7, 4, 2, 7);
    }
    if open(room, Direction::West) {
        generate_water_box(placer, 0, 1, 3, 0, 2, 4);
    }
    if open(room, Direction::East) {
        generate_water_box(placer, 7, 1, 3, 7, 2, 4);
    }
}

fn place_simple_top_room(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    random: &mut WorldgenRandom,
    room: OceanMonumentRoomData,
) {
    if room.index / 25 > 0 {
        generate_default_floor(placer, 0, 0, open(room, Direction::Down));
    }
    if !room.has_up_connection {
        generate_box_on_fill_only(placer, 1, 4, 1, 6, 4, 6, base_gray());
    }

    let wet_sponge = vanilla_blocks::WET_SPONGE.default_state();
    for x in 1..=6 {
        for z in 1..=6 {
            if random.next_i32_bounded(3) != 0 {
                let y0 = 2 + i32::from(random.next_i32_bounded(4) != 0);
                placer.generate_box(x, y0, z, x, 3, z, wet_sponge, wet_sponge, false);
            }
        }
    }

    place_simple_room_design2(placer, room);
}

fn place_wing_room(placer: &mut ScatteredFeaturePlacer<'_, '_>, main_design: i32) {
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

fn place_penthouse(placer: &mut ScatteredFeaturePlacer<'_, '_>) {
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

fn generate_water_box(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
) {
    let water = vanilla_blocks::WATER.default_state();
    let air = vanilla_blocks::AIR.default_state();
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                let block = placer.block_at(x, y, z);
                if fill_keeps(block) {
                    continue;
                }

                let pos = placer.world_pos(x, y, z);
                if pos.y() >= placer.sea_level() && block != water {
                    placer.place_block(air, x, y, z);
                } else {
                    placer.place_block(water, x, y, z);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "matches vanilla OceanMonumentPiece.generateBoxOnFillOnly bounds"
)]
fn generate_box_on_fill_only(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    x0: i32,
    y0: i32,
    z0: i32,
    x1: i32,
    y1: i32,
    z1: i32,
    state: BlockStateId,
) {
    let water = vanilla_blocks::WATER.default_state();
    for y in y0..=y1 {
        for x in x0..=x1 {
            for z in z0..=z1 {
                if placer.block_at(x, y, z) == water {
                    placer.place_block(state, x, y, z);
                }
            }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "direct port of vanilla OceanMonumentPiece.generateDefaultFloor"
)]
fn generate_default_floor(
    placer: &mut ScatteredFeaturePlacer<'_, '_>,
    xoff: i32,
    zoff: i32,
    down_opening: bool,
) {
    if down_opening {
        placer.generate_box(
            xoff,
            0,
            zoff,
            xoff + 2,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 5,
            0,
            zoff,
            xoff + 7,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff,
            xoff + 4,
            0,
            zoff + 2,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 5,
            xoff + 4,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 2,
            xoff + 4,
            0,
            zoff + 2,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 3,
            0,
            zoff + 5,
            xoff + 4,
            0,
            zoff + 5,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 2,
            0,
            zoff + 3,
            xoff + 2,
            0,
            zoff + 4,
            base_light(),
            base_light(),
            false,
        );
        placer.generate_box(
            xoff + 5,
            0,
            zoff + 3,
            xoff + 5,
            0,
            zoff + 4,
            base_light(),
            base_light(),
            false,
        );
    } else {
        placer.generate_box(
            xoff,
            0,
            zoff,
            xoff + 7,
            0,
            zoff + 7,
            base_gray(),
            base_gray(),
            false,
        );
    }
}

fn spawn_elder(placer: &mut ScatteredFeaturePlacer<'_, '_>, x: i32, y: i32, z: i32) {
    let pos = placer.world_pos(x, y, z);
    if !placer.clip().is_inside(pos) {
        return;
    }

    let entity = Arc::new(RawEntity::new(
        next_entity_id(),
        DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        ),
        placer.weak_world(),
        &vanilla_entities::ELDER_GUARDIAN,
    ));
    entity.set_persistence_required();
    entity.snap_to(
        DVec3::new(
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()),
            f64::from(pos.z()) + 0.5,
        ),
        0.0,
        0.0,
    );
    let _ = placer.add_fresh_entity(entity);
}

fn fill_keeps(state: BlockStateId) -> bool {
    let block = state.get_block();
    block == &vanilla_blocks::ICE
        || block == &vanilla_blocks::PACKED_ICE
        || block == &vanilla_blocks::BLUE_ICE
        || block == &vanilla_blocks::WATER
}

const fn open(room: OceanMonumentRoomData, direction: Direction) -> bool {
    room.has_opening[direction_index(direction)]
}

const fn direction_index(direction: Direction) -> usize {
    match direction {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

fn base_gray() -> BlockStateId {
    vanilla_blocks::PRISMARINE.default_state()
}

fn base_light() -> BlockStateId {
    vanilla_blocks::PRISMARINE_BRICKS.default_state()
}

fn base_black() -> BlockStateId {
    vanilla_blocks::DARK_PRISMARINE.default_state()
}

fn dot_deco() -> BlockStateId {
    base_light()
}

fn lamp() -> BlockStateId {
    vanilla_blocks::SEA_LANTERN.default_state()
}
