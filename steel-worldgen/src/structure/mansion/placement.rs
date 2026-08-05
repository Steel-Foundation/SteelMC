use super::{Direction, IVec3, LegacyRandom, Random, Rotation};
use super::{
    grid::{
        MansionGrid, ROOM_1X1, ROOM_1X2, ROOM_2X2, ROOM_CORRIDOR_FLAG, ROOM_DOOR_FLAG,
        ROOM_ID_MASK, ROOM_ORIGIN_FLAG, ROOM_STAIRS_FLAG, ROOM_TYPE_MASK, is_house,
    },
    roof::create_roof,
    rooms::{add_room_1x1, add_room_1x2, add_room_2x2, add_room_2x2_secret},
    template::{MansionTemplatePiece, Mirror, above, add_piece, compose_rotation, relative},
    walls::{place_entrance, traverse_outer_walls, traverse_wall_piece},
};

#[expect(
    clippy::match_same_arms,
    reason = "table kept one-per-case to match vanilla's FirstFloorRoomCollection / SecondFloor / ThirdFloor dispatch"
)]
pub(super) fn get_room_name(
    rng: &mut LegacyRandom,
    floor: usize,
    kind: &str,
    is_stairs: bool,
) -> String {
    match (floor, kind) {
        (0, "1x1") => format!("1x1_a{}", rng.next_i32_bounded(5) + 1),
        (0, "1x1s") => format!("1x1_as{}", rng.next_i32_bounded(4) + 1),
        (0, "1x2side") => format!("1x2_a{}", rng.next_i32_bounded(9) + 1),
        (0, "1x2front") => format!("1x2_b{}", rng.next_i32_bounded(5) + 1),
        (0, "1x2secret") => format!("1x2_s{}", rng.next_i32_bounded(2) + 1),
        (0, "2x2") => format!("2x2_a{}", rng.next_i32_bounded(4) + 1),
        (0, "2x2secret") => "2x2_s1".to_string(),
        (_, "1x1") => format!("1x1_b{}", rng.next_i32_bounded(5) + 1),
        (_, "1x1s") => format!("1x1_as{}", rng.next_i32_bounded(4) + 1),
        (_, "1x2side") => {
            if is_stairs {
                "1x2_c_stairs".to_string()
            } else {
                format!("1x2_c{}", rng.next_i32_bounded(4) + 1)
            }
        }
        (_, "1x2front") => {
            if is_stairs {
                "1x2_d_stairs".to_string()
            } else {
                format!("1x2_d{}", rng.next_i32_bounded(5) + 1)
            }
        }
        (_, "1x2secret") => format!("1x2_se{}", rng.next_i32_bounded(1) + 1),
        (_, "2x2") => format!("2x2_b{}", rng.next_i32_bounded(5) + 1),
        (_, "2x2secret") => "2x2_s1".to_string(),
        _ => "corridor_floor".to_string(),
    }
}

pub(super) struct PlacementData {
    pub(super) position: IVec3,
    pub(super) rotation: Rotation,
    pub(super) wall_type: &'static str,
}

#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's MansionPiecePlacer traversal order"
)]
pub(super) fn generate_mansion_pieces(
    origin: IVec3,
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<MansionTemplatePiece> {
    let mansion = MansionGrid::new(rng);
    let start_x = mansion.entrance_x + 1;
    let start_y = mansion.entrance_y + 1;
    let end_x = mansion.entrance_x + 1;
    let end_y = mansion.entrance_y;

    let mut pieces: Vec<MansionTemplatePiece> = Vec::new();

    let mut data = PlacementData {
        position: origin,
        rotation,
        wall_type: "wall_flat",
    };
    place_entrance(&mut pieces, &mut data);

    let mut second = PlacementData {
        position: above(data.position, 8),
        rotation: data.rotation,
        wall_type: "wall_window",
    };

    traverse_outer_walls(
        &mut pieces,
        &mut data,
        &mansion.base_grid,
        Direction::South,
        start_x,
        start_y,
        end_x,
        end_y,
    );
    traverse_outer_walls(
        &mut pieces,
        &mut second,
        &mansion.base_grid,
        Direction::South,
        start_x,
        start_y,
        end_x,
        end_y,
    );

    let mut third_data = PlacementData {
        position: above(data.position, 19),
        rotation: data.rotation,
        wall_type: "wall_window",
    };

    let mut done = false;
    for y in 0..mansion.third_floor_grid.height {
        if done {
            break;
        }
        for x in (0..mansion.third_floor_grid.width).rev() {
            if done {
                break;
            }
            if is_house(&mansion.third_floor_grid, x, y) {
                third_data.position = relative(
                    third_data.position,
                    rotation,
                    Direction::South,
                    8 + (y - start_y) * 8,
                );
                third_data.position = relative(
                    third_data.position,
                    rotation,
                    Direction::East,
                    (x - start_x) * 8,
                );
                traverse_wall_piece(&mut pieces, &mut third_data);
                traverse_outer_walls(
                    &mut pieces,
                    &mut third_data,
                    &mansion.third_floor_grid,
                    Direction::South,
                    x,
                    y,
                    x,
                    y,
                );
                done = true;
            }
        }
    }

    create_roof(
        &mut pieces,
        above(origin, 16),
        rotation,
        &mansion.base_grid,
        Some(&mansion.third_floor_grid),
        start_x,
        start_y,
    );
    create_roof(
        &mut pieces,
        above(origin, 27),
        rotation,
        &mansion.third_floor_grid,
        None,
        start_x,
        start_y,
    );

    for floor_num in 0..3_usize {
        let floor_origin = above(
            origin,
            8 * floor_num as i32 + if floor_num == 2 { 3 } else { 0 },
        );
        let rooms = &mansion.floor_rooms[floor_num];
        let grid = if floor_num == 2 {
            &mansion.third_floor_grid
        } else {
            &mansion.base_grid
        };
        let south_piece = if floor_num == 0 {
            "carpet_south_1"
        } else {
            "carpet_south_2"
        };
        let west_piece = if floor_num == 0 {
            "carpet_west_1"
        } else {
            "carpet_west_2"
        };

        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) == 1 {
                    let mut pos = relative(
                        floor_origin,
                        rotation,
                        Direction::South,
                        8 + (y - start_y) * 8,
                    );
                    pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
                    add_piece(&mut pieces, "corridor_floor", pos, rotation, Mirror::None);

                    if grid.get(x, y - 1) == 1 || (rooms.get(x, y - 1) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = above(
                            relative(
                                relative(pos, rotation, Direction::East, 1),
                                rotation,
                                Direction::South,
                                0,
                            ),
                            1,
                        );
                        add_piece(&mut pieces, "carpet_north", p, rotation, Mirror::None);
                    }
                    if grid.get(x + 1, y) == 1 || (rooms.get(x + 1, y) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = above(
                            relative(
                                relative(pos, rotation, Direction::South, 1),
                                rotation,
                                Direction::East,
                                5,
                            ),
                            1,
                        );
                        add_piece(&mut pieces, "carpet_east", p, rotation, Mirror::None);
                    }
                    if grid.get(x, y + 1) == 1 || (rooms.get(x, y + 1) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = relative(
                            relative(pos, rotation, Direction::South, 5),
                            rotation,
                            Direction::West,
                            1,
                        );
                        add_piece(&mut pieces, south_piece, p, rotation, Mirror::None);
                    }
                    if grid.get(x - 1, y) == 1 || (rooms.get(x - 1, y) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 1),
                            rotation,
                            Direction::North,
                            1,
                        );
                        add_piece(&mut pieces, west_piece, p, rotation, Mirror::None);
                    }
                }
            }
        }

        let wall_piece = if floor_num == 0 {
            "indoors_wall_1"
        } else {
            "indoors_wall_2"
        };
        let door_piece = if floor_num == 0 {
            "indoors_door_1"
        } else {
            "indoors_door_2"
        };

        for y in 0..grid.height {
            for x in 0..grid.width {
                let is_third_start = floor_num == 2 && grid.get(x, y) == 3;
                if grid.get(x, y) != 2 && !is_third_start {
                    continue;
                }
                let room_data = rooms.get(x, y);
                let room_type = room_data & ROOM_TYPE_MASK;
                let room_id = room_data & ROOM_ID_MASK;
                let is_corridor_start = is_third_start && (room_data & ROOM_CORRIDOR_FLAG) != 0;

                let mut door_dirs: Vec<Direction> = Vec::new();
                if (room_data & ROOM_DOOR_FLAG) != 0 {
                    for dir in Direction::HORIZONTAL {
                        let (ox, oz) = dir.offset_xz();
                        if grid.get(x + ox, y + oz) == 1 {
                            door_dirs.push(dir);
                        }
                    }
                }

                let door_dir: Option<Direction> = if !door_dirs.is_empty() {
                    Some(door_dirs[rng.next_i32_bounded(door_dirs.len() as i32) as usize])
                } else if (room_data & ROOM_ORIGIN_FLAG) != 0 {
                    Some(Direction::Up)
                } else {
                    None
                };

                let mut room_pos = relative(
                    floor_origin,
                    rotation,
                    Direction::South,
                    8 + (y - start_y) * 8,
                );
                room_pos = relative(room_pos, rotation, Direction::East, -1 + (x - start_x) * 8);

                if is_house(grid, x - 1, y) && !mansion.is_room_id(x - 1, y, floor_num, room_id) {
                    let template = if door_dir == Some(Direction::West) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    add_piece(&mut pieces, template, room_pos, rotation, Mirror::None);
                }

                if grid.get(x + 1, y) == 1 && !is_corridor_start {
                    let p = relative(room_pos, rotation, Direction::East, 8);
                    let template = if door_dir == Some(Direction::East) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    add_piece(&mut pieces, template, p, rotation, Mirror::None);
                }

                if is_house(grid, x, y + 1) && !mansion.is_room_id(x, y + 1, floor_num, room_id) {
                    let p = relative(
                        relative(room_pos, rotation, Direction::South, 7),
                        rotation,
                        Direction::East,
                        7,
                    );
                    let template = if door_dir == Some(Direction::South) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    add_piece(
                        &mut pieces,
                        template,
                        p,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                }

                if grid.get(x, y - 1) == 1 && !is_corridor_start {
                    let p = relative(
                        relative(room_pos, rotation, Direction::North, 1),
                        rotation,
                        Direction::East,
                        7,
                    );
                    let template = if door_dir == Some(Direction::North) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    add_piece(
                        &mut pieces,
                        template,
                        p,
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    );
                }

                if room_type == ROOM_1X1 {
                    add_room_1x1(&mut pieces, room_pos, rotation, door_dir, floor_num, rng);
                } else if room_type == ROOM_1X2 && door_dir.is_some() {
                    let room_dir = mansion.get_1x2_room_direction(x, y, floor_num, room_id);
                    let is_stairs = (room_data & ROOM_STAIRS_FLAG) != 0;
                    if let (Some(rd), Some(dd)) = (room_dir, door_dir) {
                        add_room_1x2(
                            &mut pieces,
                            room_pos,
                            rotation,
                            rd,
                            dd,
                            floor_num,
                            is_stairs,
                            rng,
                        );
                    }
                } else if let (ROOM_2X2, Some(dd)) = (room_type, door_dir)
                    && dd != Direction::Up
                {
                    let mut room_dir = dd.rotate_y_clockwise();
                    let (ox, oz) = room_dir.offset_xz();
                    if !mansion.is_room_id(x + ox, y + oz, floor_num, room_id) {
                        room_dir = room_dir.opposite();
                    }
                    add_room_2x2(
                        &mut pieces,
                        room_pos,
                        rotation,
                        room_dir,
                        dd,
                        floor_num,
                        rng,
                    );
                } else if room_type == ROOM_2X2 && door_dir == Some(Direction::Up) {
                    add_room_2x2_secret(&mut pieces, room_pos, rotation, floor_num, rng);
                }
            }
        }
    }

    pieces
}
