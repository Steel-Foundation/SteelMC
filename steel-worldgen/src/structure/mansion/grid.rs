use super::template::dir_from_2d;
use super::{Direction, LegacyRandom, Random};

pub(super) struct SimpleGrid {
    pub(super) grid: Vec<Vec<i32>>,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) outside: i32,
}

impl SimpleGrid {
    pub(super) fn new(width: i32, height: i32, outside: i32) -> Self {
        Self {
            grid: vec![vec![0; height as usize]; width as usize],
            width,
            height,
            outside,
        }
    }

    pub(super) fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[x as usize][y as usize]
        } else {
            self.outside
        }
    }

    pub(super) fn set_cell(&mut self, x: i32, y: i32, value: i32) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[x as usize][y as usize] = value;
        }
    }

    pub(super) fn set_range(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, value: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set_cell(x, y, value);
            }
        }
    }

    pub(super) fn setif(&mut self, x: i32, y: i32, if_value: i32, value: i32) {
        if self.get(x, y) == if_value {
            self.set_cell(x, y, value);
        }
    }

    pub(super) fn edges_to(&self, x: i32, y: i32, value: i32) -> bool {
        self.get(x - 1, y) == value
            || self.get(x + 1, y) == value
            || self.get(x, y + 1) == value
            || self.get(x, y - 1) == value
    }
}

pub(super) fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    let v = grid.get(x, y);
    v == 1 || v == 2 || v == 3 || v == 4
}

pub(super) const ROOM_1X1: i32 = 65_536;
pub(super) const ROOM_1X2: i32 = 131_072;
pub(super) const ROOM_2X2: i32 = 262_144;
pub(super) const ROOM_ORIGIN_FLAG: i32 = 1_048_576;
pub(super) const ROOM_DOOR_FLAG: i32 = 2_097_152;
pub(super) const ROOM_STAIRS_FLAG: i32 = 4_194_304;
pub(super) const ROOM_CORRIDOR_FLAG: i32 = 8_388_608;
pub(super) const ROOM_TYPE_MASK: i32 = 983_040;
pub(super) const ROOM_ID_MASK: i32 = 65_535;

pub(super) struct MansionGrid {
    pub(super) base_grid: SimpleGrid,
    pub(super) third_floor_grid: SimpleGrid,
    pub(super) floor_rooms: [SimpleGrid; 3],
    pub(super) entrance_x: i32,
    pub(super) entrance_y: i32,
}

impl MansionGrid {
    pub(super) fn new(rng: &mut LegacyRandom) -> Self {
        let entrance_x = 7;
        let entrance_y = 4;
        let mut base = SimpleGrid::new(11, 11, 5);
        base.set_range(entrance_x, entrance_y, entrance_x + 1, entrance_y + 1, 3);
        base.set_range(
            entrance_x - 1,
            entrance_y,
            entrance_x - 1,
            entrance_y + 1,
            2,
        );
        base.set_range(
            entrance_x + 2,
            entrance_y - 2,
            entrance_x + 3,
            entrance_y + 3,
            5,
        );
        base.set_range(
            entrance_x + 1,
            entrance_y - 2,
            entrance_x + 1,
            entrance_y - 1,
            1,
        );
        base.set_range(
            entrance_x + 1,
            entrance_y + 2,
            entrance_x + 1,
            entrance_y + 3,
            1,
        );
        base.set_cell(entrance_x - 1, entrance_y - 1, 1);
        base.set_cell(entrance_x - 1, entrance_y + 2, 1);
        base.set_range(0, 0, 11, 1, 5);
        base.set_range(0, 9, 11, 11, 5);
        for (x, y, depth) in [
            (entrance_x, entrance_y - 2, 6),
            (entrance_x, entrance_y + 3, 6),
            (entrance_x - 2, entrance_y - 1, 3),
            (entrance_x - 2, entrance_y + 2, 3),
        ] {
            Self::recursive_corridor(&mut base, rng, x, y, Direction::West, depth);
        }
        while Self::clean_edges(&mut base) {}

        let mut floor_rooms = [
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
            SimpleGrid::new(11, 11, 5),
        ];
        Self::identify_rooms(&base, &mut floor_rooms[0], rng);
        Self::identify_rooms(&base, &mut floor_rooms[1], rng);
        for room in &mut floor_rooms[0..2] {
            room.set_range(
                entrance_x + 1,
                entrance_y,
                entrance_x + 1,
                entrance_y + 1,
                ROOM_CORRIDOR_FLAG,
            );
        }

        let mut third = SimpleGrid::new(base.width, base.height, 5);
        Self::setup_third_floor(&base, &mut third, &mut floor_rooms, rng);
        Self::identify_rooms(&third, &mut floor_rooms[2], rng);

        Self {
            base_grid: base,
            third_floor_grid: third,
            floor_rooms,
            entrance_x,
            entrance_y,
        }
    }

    pub(super) fn recursive_corridor(
        grid: &mut SimpleGrid,
        rng: &mut LegacyRandom,
        x: i32,
        y: i32,
        heading: Direction,
        depth: i32,
    ) {
        if depth <= 0 {
            return;
        }
        grid.set_cell(x, y, 1);
        let (hx, hz) = heading.offset_xz();
        grid.setif(x + hx, y + hz, 0, 1);

        for _ in 0..8 {
            let next_dir = dir_from_2d(rng.next_i32_bounded(4));
            if next_dir == heading.opposite() || (next_dir == Direction::East && rng.next_bool()) {
                continue;
            }
            let (nx, ny) = (x + hx, y + hz);
            let (ndx, ndz) = next_dir.offset_xz();
            if grid.get(nx + ndx, ny + ndz) == 0 && grid.get(nx + ndx * 2, ny + ndz * 2) == 0 {
                Self::recursive_corridor(
                    grid,
                    rng,
                    x + hx + ndx,
                    y + hz + ndz,
                    next_dir,
                    depth - 1,
                );
                break;
            }
        }

        let cw = heading.rotate_y_clockwise();
        let ccw = heading.rotate_y_counter_clockwise();
        let a_cw_off = cw.offset_vec();
        let b_ccw_off = ccw.offset_vec();
        grid.setif(x + a_cw_off.x, y + a_cw_off.z, 0, 2);
        grid.setif(x + b_ccw_off.x, y + b_ccw_off.z, 0, 2);
        grid.setif(x + hx + a_cw_off.x, y + hz + a_cw_off.z, 0, 2);
        grid.setif(x + hx + b_ccw_off.x, y + hz + b_ccw_off.z, 0, 2);
        grid.setif(x + hx * 2, y + hz * 2, 0, 2);
        grid.setif(x + a_cw_off.x * 2, y + a_cw_off.z * 2, 0, 2);
        grid.setif(x + b_ccw_off.x * 2, y + b_ccw_off.z * 2, 0, 2);
    }

    pub(super) fn clean_edges(grid: &mut SimpleGrid) -> bool {
        let mut touched = false;
        for y in 0..grid.height {
            for x in 0..grid.width {
                if grid.get(x, y) != 0 {
                    continue;
                }
                let direct = i32::from(is_house(grid, x + 1, y))
                    + i32::from(is_house(grid, x - 1, y))
                    + i32::from(is_house(grid, x, y + 1))
                    + i32::from(is_house(grid, x, y - 1));
                if direct >= 3 {
                    grid.set_cell(x, y, 2);
                    touched = true;
                } else if direct == 2 {
                    let diag = i32::from(is_house(grid, x + 1, y + 1))
                        + i32::from(is_house(grid, x - 1, y + 1))
                        + i32::from(is_house(grid, x + 1, y - 1))
                        + i32::from(is_house(grid, x - 1, y - 1));
                    if diag <= 1 {
                        grid.set_cell(x, y, 2);
                        touched = true;
                    }
                }
            }
        }
        touched
    }

    pub(super) fn identify_rooms(
        from: &SimpleGrid,
        rooms: &mut SimpleGrid,
        rng: &mut LegacyRandom,
    ) {
        let mut positions: Vec<(i32, i32)> = Vec::new();
        for y in 0..from.height {
            for x in 0..from.width {
                if from.get(x, y) == 2 {
                    positions.push((x, y));
                }
            }
        }
        let len = positions.len();
        for i in (1..len).rev() {
            let j = rng.next_i32_bounded((i + 1) as i32) as usize;
            positions.swap(i, j);
        }

        let mut room_id = 10;
        for &(x, y) in &positions {
            if rooms.get(x, y) != 0 {
                continue;
            }
            let (mut x0, mut x1, mut y0, mut y1) = (x, x, y, y);
            let mut rtype = ROOM_1X1;

            if rooms.get(x + 1, y) == 0
                && rooms.get(x, y + 1) == 0
                && rooms.get(x + 1, y + 1) == 0
                && from.get(x + 1, y) == 2
                && from.get(x, y + 1) == 2
                && from.get(x + 1, y + 1) == 2
            {
                x1 = x + 1;
                y1 = y + 1;
                rtype = ROOM_2X2;
            } else if rooms.get(x - 1, y) == 0
                && rooms.get(x, y + 1) == 0
                && rooms.get(x - 1, y + 1) == 0
                && from.get(x - 1, y) == 2
                && from.get(x, y + 1) == 2
                && from.get(x - 1, y + 1) == 2
            {
                x0 = x - 1;
                y1 = y + 1;
                rtype = ROOM_2X2;
            } else if rooms.get(x - 1, y) == 0
                && rooms.get(x, y - 1) == 0
                && rooms.get(x - 1, y - 1) == 0
                && from.get(x - 1, y) == 2
                && from.get(x, y - 1) == 2
                && from.get(x - 1, y - 1) == 2
            {
                x0 = x - 1;
                y0 = y - 1;
                rtype = ROOM_2X2;
            } else if rooms.get(x + 1, y) == 0 && from.get(x + 1, y) == 2 {
                x1 = x + 1;
                rtype = ROOM_1X2;
            } else if rooms.get(x, y + 1) == 0 && from.get(x, y + 1) == 2 {
                y1 = y + 1;
                rtype = ROOM_1X2;
            } else if rooms.get(x - 1, y) == 0 && from.get(x - 1, y) == 2 {
                x0 = x - 1;
                rtype = ROOM_1X2;
            } else if rooms.get(x, y - 1) == 0 && from.get(x, y - 1) == 2 {
                y0 = y - 1;
                rtype = ROOM_1X2;
            }

            let mut door_x = if rng.next_bool() { x0 } else { x1 };
            let mut door_y = if rng.next_bool() { y0 } else { y1 };
            let mut door_flag = ROOM_DOOR_FLAG;
            if !from.edges_to(door_x, door_y, 1) {
                door_x = if door_x == x0 { x1 } else { x0 };
                door_y = if door_y == y0 { y1 } else { y0 };
                if !from.edges_to(door_x, door_y, 1) {
                    door_y = if door_y == y0 { y1 } else { y0 };
                    if !from.edges_to(door_x, door_y, 1) {
                        door_x = if door_x == x0 { x1 } else { x0 };
                        door_y = if door_y == y0 { y1 } else { y0 };
                        if !from.edges_to(door_x, door_y, 1) {
                            door_flag = 0;
                            door_x = x0;
                            door_y = y0;
                        }
                    }
                }
            }

            for ry in y0..=y1 {
                for rx in x0..=x1 {
                    if rx == door_x && ry == door_y {
                        rooms.set_cell(rx, ry, ROOM_ORIGIN_FLAG | door_flag | rtype | room_id);
                    } else {
                        rooms.set_cell(rx, ry, rtype | room_id);
                    }
                }
            }
            room_id += 1;
        }
    }

    pub(super) fn setup_third_floor(
        base: &SimpleGrid,
        third: &mut SimpleGrid,
        floor_rooms: &mut [SimpleGrid; 3],
        rng: &mut LegacyRandom,
    ) {
        let mut potential: Vec<(i32, i32)> = Vec::new();
        for y in 0..third.height {
            for x in 0..third.width {
                let data = floor_rooms[1].get(x, y);
                if (data & ROOM_TYPE_MASK) == ROOM_1X2 && (data & ROOM_DOOR_FLAG) != 0 {
                    potential.push((x, y));
                }
            }
        }

        if potential.is_empty() {
            third.set_range(0, 0, third.width, third.height, 5);
            return;
        }

        let &(rx, ry) = &potential[rng.next_i32_bounded(potential.len() as i32) as usize];
        let room_data = floor_rooms[1].get(rx, ry);
        floor_rooms[1].set_cell(rx, ry, room_data | ROOM_STAIRS_FLAG);

        let room_id = room_data & ROOM_ID_MASK;
        let room_dir = Self::get_1x2_room_direction_static(&floor_rooms[1], rx, ry, room_id);
        let (rex, rey) = match room_dir {
            Some(d) => {
                let off = d.offset_vec();
                (rx + off.x, ry + off.z)
            }
            None => (rx, ry),
        };

        for y in 0..third.height {
            for x in 0..third.width {
                if !is_house(base, x, y) {
                    third.set_cell(x, y, 5);
                } else if x == rx && y == ry {
                    third.set_cell(x, y, 3);
                } else if x == rex && y == rey {
                    third.set_cell(x, y, 3);
                    floor_rooms[2].set_cell(x, y, ROOM_CORRIDOR_FLAG);
                }
            }
        }

        let mut potential_dirs: Vec<Direction> = Vec::new();
        for dir in Direction::HORIZONTAL {
            let (ox, oz) = dir.offset_xz();
            if third.get(rex + ox, rey + oz) == 0 {
                potential_dirs.push(dir);
            }
        }

        if potential_dirs.is_empty() {
            third.set_range(0, 0, third.width, third.height, 5);
            floor_rooms[1].set_cell(rx, ry, room_data);
        } else {
            let corridor_dir =
                potential_dirs[rng.next_i32_bounded(potential_dirs.len() as i32) as usize];
            let (ox, oz) = corridor_dir.offset_xz();
            Self::recursive_corridor(third, rng, rex + ox, rey + oz, corridor_dir, 4);
            while Self::clean_edges(third) {}
        }
    }

    pub(super) fn is_room_id(&self, x: i32, y: i32, floor: usize, room_id: i32) -> bool {
        (self.floor_rooms[floor].get(x, y) & ROOM_ID_MASK) == room_id
    }

    pub(super) fn get_1x2_room_direction(
        &self,
        x: i32,
        y: i32,
        floor: usize,
        room_id: i32,
    ) -> Option<Direction> {
        for dir in &[
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let (ox, oz) = dir.offset_xz();
            if self.is_room_id(x + ox, y + oz, floor, room_id) {
                return Some(*dir);
            }
        }
        None
    }

    pub(super) fn get_1x2_room_direction_static(
        floor_rooms: &SimpleGrid,
        x: i32,
        y: i32,
        room_id: i32,
    ) -> Option<Direction> {
        for dir in &[
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            let (ox, oz) = dir.offset_xz();
            if (floor_rooms.get(x + ox, y + oz) & ROOM_ID_MASK) == room_id {
                return Some(*dir);
            }
        }
        None
    }
}
