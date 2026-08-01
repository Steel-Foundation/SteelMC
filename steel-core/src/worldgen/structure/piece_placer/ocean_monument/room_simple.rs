use super::{
    Direction, OceanMonumentRoomData, Random, ScatteredFeaturePlacer, WorldgenRandom, base_black,
    base_gray, base_light, generate_box_on_fill_only, generate_default_floor, generate_water_box,
    lamp, open, vanilla_blocks,
};

pub(super) fn place_simple_room(
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

pub(super) fn place_simple_top_room(
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
