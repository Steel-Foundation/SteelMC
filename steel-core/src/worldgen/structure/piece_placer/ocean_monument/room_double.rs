use super::{
    Direction, OceanMonumentRoomData, ScatteredFeaturePlacer, base_black, base_gray, base_light,
    generate_box_on_fill_only, generate_default_floor, generate_water_box, lamp, open,
};

pub(super) fn place_double_x_room(
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

pub(super) fn place_double_xy_room(
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

pub(super) fn place_double_y_room(
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

pub(super) fn place_double_yz_room(
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

pub(super) fn place_double_z_room(
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
