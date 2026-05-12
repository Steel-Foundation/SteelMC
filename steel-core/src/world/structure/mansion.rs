//! Woodland mansion. Vanilla's `WoodlandMansionPieces`: grid-based layout with
//! template pieces for walls, corridors, rooms, roofs. Produces bounding boxes only.

use steel_registry::structure::StructureData;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier, Rotation};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
};

/// (sizeX, sizeY, sizeZ) for a `woodland_mansion` template.
fn template_size(name: &str) -> [i32; 3] {
    match name {
        "entrance" => [21, 19, 16],
        "wall_flat" | "wall_window" => [2, 8, 8],
        "wall_corner" => [9, 8, 2],
        "corridor_floor" => [7, 8, 7],
        "carpet_north" => [5, 1, 2],
        "carpet_east" => [2, 1, 5],
        "carpet_south_1" => [8, 8, 3],
        "carpet_south_2" => [8, 11, 3],
        "carpet_west_1" => [3, 8, 8],
        "carpet_west_2" => [3, 11, 8],
        "indoors_wall_1" | "indoors_door_1" => [1, 8, 8],
        "indoors_wall_2" | "indoors_door_2" => [1, 11, 8],
        "roof" => [8, 1, 8],
        "roof_corner" | "roof_inner_corner" => [4, 4, 4],
        "roof_front" => [4, 4, 8],
        "small_wall" => [2, 4, 8],
        "small_wall_corner" => [2, 4, 2],
        // 1x1 rooms (floor 1: 8 high, floor 2+: 11 high)
        s if s.starts_with("1x1_a") => [7, 8, 7],
        s if s.starts_with("1x1_b") => [7, 11, 7],
        // 1x2 rooms
        "1x2_c_stairs" | "1x2_d_stairs" => [7, 22, 15],
        s if s.starts_with("1x2_c") || s.starts_with("1x2_d") || s.starts_with("1x2_se") => {
            [7, 11, 15]
        }
        s if s.starts_with("1x2_a") || s.starts_with("1x2_b") || s.starts_with("1x2_s") => {
            [7, 8, 15]
        }
        // 2x2 rooms
        s if s.starts_with("2x2_a") => [15, 8, 15],
        s if s.starts_with("2x2_b") || s.starts_with("2x2_s") => [15, 11, 15],
        _ => {
            tracing::warn!("Unknown mansion template: {name}, using 1x1x1");
            [1, 1, 1]
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mirror {
    None,
    LeftRight,
    FrontBack,
}

fn piece_bb(
    pos: (i32, i32, i32),
    size: [i32; 3],
    rotation: Rotation,
    mirror: Mirror,
) -> BoundingBox {
    let (dx, dy, dz) = (size[0] - 1, size[1] - 1, size[2] - 1);
    let (x1, z1) = apply_mirror(0, 0, mirror);
    let (x2, z2) = apply_mirror(dx, dz, mirror);
    let (c1x, c1y, c1z) = rotation.transform_pos(x1, 0, z1, 0, 0);
    let (c2x, c2y, c2z) = rotation.transform_pos(x2, dy, z2, 0, 0);
    BoundingBox::new(
        c1x.min(c2x) + pos.0,
        c1y.min(c2y) + pos.1,
        c1z.min(c2z) + pos.2,
        c1x.max(c2x) + pos.0,
        c1y.max(c2y) + pos.1,
        c1z.max(c2z) + pos.2,
    )
}

const fn apply_mirror(x: i32, z: i32, mirror: Mirror) -> (i32, i32) {
    match mirror {
        Mirror::None => (x, z),
        Mirror::FrontBack => (-x, z),
        Mirror::LeftRight => (x, -z),
    }
}

/// `pos.relative(rotation.rotate(direction), amount)`.
const fn relative(
    pos: (i32, i32, i32),
    rotation: Rotation,
    dir: Direction,
    amount: i32,
) -> (i32, i32, i32) {
    let rotated = rotation.rotate(dir);
    let (dx, dy, dz) = rotated.offset();
    (
        pos.0 + dx * amount,
        pos.1 + dy * amount,
        pos.2 + dz * amount,
    )
}

const fn above(pos: (i32, i32, i32), amount: i32) -> (i32, i32, i32) {
    (pos.0, pos.1 + amount, pos.2)
}

/// Vanilla's `getZeroPositionWithTransform(zeroPos, Mirror.NONE, rotation, sx, sz)`.
const fn zero_pos_transform(
    zero: (i32, i32, i32),
    rotation: Rotation,
    size_x: i32,
    size_z: i32,
) -> (i32, i32, i32) {
    let sx = size_x - 1;
    let sz = size_z - 1;
    let (dx, dz) = match rotation {
        Rotation::None => (0, 0),
        Rotation::Clockwise90 => (sz, 0),
        Rotation::Clockwise180 => (sx, sz),
        Rotation::CounterClockwise90 => (0, sx),
    };
    (zero.0 + dx, zero.1, zero.2 + dz)
}

/// Vanilla's `Rotation.getRotated`.
const fn compose_rotation(base: Rotation, add: Rotation) -> Rotation {
    base.then(add)
}

/// Vanilla's `Direction.from2DDataValue`: 0=S, 1=W, 2=N, 3=E.
const fn dir_from_2d(value: i32) -> Direction {
    match value & 3 {
        0 => Direction::South,
        1 => Direction::West,
        2 => Direction::North,
        _ => Direction::East,
    }
}

struct SimpleGrid {
    grid: Vec<Vec<i32>>,
    width: i32,
    height: i32,
    outside: i32,
}

impl SimpleGrid {
    fn new(width: i32, height: i32, outside: i32) -> Self {
        Self {
            grid: vec![vec![0; height as usize]; width as usize],
            width,
            height,
            outside,
        }
    }

    fn get(&self, x: i32, y: i32) -> i32 {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[x as usize][y as usize]
        } else {
            self.outside
        }
    }

    fn set_cell(&mut self, x: i32, y: i32, value: i32) {
        if x >= 0 && x < self.width && y >= 0 && y < self.height {
            self.grid[x as usize][y as usize] = value;
        }
    }

    fn set_range(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, value: i32) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set_cell(x, y, value);
            }
        }
    }

    fn setif(&mut self, x: i32, y: i32, if_value: i32, value: i32) {
        if self.get(x, y) == if_value {
            self.set_cell(x, y, value);
        }
    }

    fn edges_to(&self, x: i32, y: i32, value: i32) -> bool {
        self.get(x - 1, y) == value
            || self.get(x + 1, y) == value
            || self.get(x, y + 1) == value
            || self.get(x, y - 1) == value
    }
}

fn is_house(grid: &SimpleGrid, x: i32, y: i32) -> bool {
    let v = grid.get(x, y);
    v == 1 || v == 2 || v == 3 || v == 4
}

const ROOM_1X1: i32 = 65_536;
const ROOM_1X2: i32 = 131_072;
const ROOM_2X2: i32 = 262_144;
const ROOM_ORIGIN_FLAG: i32 = 1_048_576;
const ROOM_DOOR_FLAG: i32 = 2_097_152;
const ROOM_STAIRS_FLAG: i32 = 4_194_304;
const ROOM_CORRIDOR_FLAG: i32 = 8_388_608;
const ROOM_TYPE_MASK: i32 = 983_040;
const ROOM_ID_MASK: i32 = 65_535;

struct MansionGrid {
    base_grid: SimpleGrid,
    third_floor_grid: SimpleGrid,
    floor_rooms: [SimpleGrid; 3],
    entrance_x: i32,
    entrance_y: i32,
}

impl MansionGrid {
    fn new(rng: &mut LegacyRandom) -> Self {
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

    fn recursive_corridor(
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
        let (hx, _, hz) = heading.offset();
        grid.setif(x + hx, y + hz, 0, 1);

        for _ in 0..8 {
            let next_dir = dir_from_2d(rng.next_i32_bounded(4));
            if next_dir == heading.opposite() || (next_dir == Direction::East && rng.next_bool()) {
                continue;
            }
            let (nx, ny) = (x + hx, y + hz);
            let (ndx, ndz) = (next_dir.offset().0, next_dir.offset().2);
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
        grid.setif(x + cw.offset().0, y + cw.offset().2, 0, 2);
        grid.setif(x + ccw.offset().0, y + ccw.offset().2, 0, 2);
        grid.setif(x + hx + cw.offset().0, y + hz + cw.offset().2, 0, 2);
        grid.setif(x + hx + ccw.offset().0, y + hz + ccw.offset().2, 0, 2);
        grid.setif(x + hx * 2, y + hz * 2, 0, 2);
        grid.setif(x + cw.offset().0 * 2, y + cw.offset().2 * 2, 0, 2);
        grid.setif(x + ccw.offset().0 * 2, y + ccw.offset().2 * 2, 0, 2);
    }

    fn clean_edges(grid: &mut SimpleGrid) -> bool {
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

    fn identify_rooms(from: &SimpleGrid, rooms: &mut SimpleGrid, rng: &mut LegacyRandom) {
        let mut positions: Vec<(i32, i32)> = Vec::new();
        for y in 0..from.height {
            for x in 0..from.width {
                if from.get(x, y) == 2 {
                    positions.push((x, y));
                }
            }
        }
        // Vanilla: Util.shuffle(roomPos, random)
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

    fn setup_third_floor(
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
            Some(d) => (rx + d.offset().0, ry + d.offset().2),
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

        // Find corridor direction from room end
        let mut potential_dirs: Vec<Direction> = Vec::new();
        for dir in &[
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            if third.get(rex + dir.offset().0, rey + dir.offset().2) == 0 {
                potential_dirs.push(*dir);
            }
        }

        if potential_dirs.is_empty() {
            third.set_range(0, 0, third.width, third.height, 5);
            floor_rooms[1].set_cell(rx, ry, room_data);
        } else {
            let corridor_dir =
                potential_dirs[rng.next_i32_bounded(potential_dirs.len() as i32) as usize];
            Self::recursive_corridor(
                third,
                rng,
                rex + corridor_dir.offset().0,
                rey + corridor_dir.offset().2,
                corridor_dir,
                4,
            );
            while Self::clean_edges(third) {}
        }
    }

    fn is_room_id(&self, x: i32, y: i32, floor: usize, room_id: i32) -> bool {
        (self.floor_rooms[floor].get(x, y) & ROOM_ID_MASK) == room_id
    }

    fn get_1x2_room_direction(
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
            if self.is_room_id(x + dir.offset().0, y + dir.offset().2, floor, room_id) {
                return Some(*dir);
            }
        }
        None
    }

    fn get_1x2_room_direction_static(
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
            if (floor_rooms.get(x + dir.offset().0, y + dir.offset().2) & ROOM_ID_MASK) == room_id {
                return Some(*dir);
            }
        }
        None
    }
}

#[expect(
    clippy::match_same_arms,
    reason = "table kept one-per-case to match vanilla's FirstFloorRoomCollection / SecondFloor / ThirdFloor dispatch"
)]
fn get_room_name(rng: &mut LegacyRandom, floor: usize, kind: &str, is_stairs: bool) -> String {
    match (floor, kind) {
        (0, "1x1") => format!("1x1_a{}", rng.next_i32_bounded(5) + 1),
        (0, "1x1s") => format!("1x1_as{}", rng.next_i32_bounded(4) + 1),
        (0, "1x2side") => format!("1x2_a{}", rng.next_i32_bounded(9) + 1),
        (0, "1x2front") => format!("1x2_b{}", rng.next_i32_bounded(5) + 1),
        (0, "1x2secret") => format!("1x2_s{}", rng.next_i32_bounded(2) + 1),
        (0, "2x2") => format!("2x2_a{}", rng.next_i32_bounded(4) + 1),
        (0, "2x2secret") => "2x2_s1".to_string(),
        // Floor 1 and 2 (ThirdFloorRoomCollection extends SecondFloorRoomCollection)
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

struct PlacementData {
    position: (i32, i32, i32),
    rotation: Rotation,
    wall_type: &'static str,
}

/// All mansion piece bounding boxes.
#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's MansionPiecePlacer traversal order"
)]
pub fn generate_mansion_pieces(
    origin: (i32, i32, i32),
    rotation: Rotation,
    rng: &mut LegacyRandom,
) -> Vec<BoundingBox> {
    let mansion = MansionGrid::new(rng);
    let start_x = mansion.entrance_x + 1;
    let start_y = mansion.entrance_y + 1;
    let end_x = mansion.entrance_x + 1;
    let end_y = mansion.entrance_y;

    let mut pieces: Vec<BoundingBox> = Vec::new();

    let mut data = PlacementData {
        position: origin,
        rotation,
        wall_type: "wall_flat",
    };
    place_entrance(&mut pieces, &mut data);

    // Capture second-floor placement BEFORE floor-0 traversal mutates `data`.
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

    // Third floor uses data.position.above(19) AFTER floor-0 traversal.
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

    // Roofs
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

    // Interior: corridors, walls, doors, rooms for 3 floors
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

        // Corridors
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
                    pieces.push(piece_bb(
                        pos,
                        template_size("corridor_floor"),
                        rotation,
                        Mirror::None,
                    ));

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
                        pieces.push(piece_bb(
                            p,
                            template_size("carpet_north"),
                            rotation,
                            Mirror::None,
                        ));
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
                        pieces.push(piece_bb(
                            p,
                            template_size("carpet_east"),
                            rotation,
                            Mirror::None,
                        ));
                    }
                    if grid.get(x, y + 1) == 1 || (rooms.get(x, y + 1) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = relative(
                            relative(pos, rotation, Direction::South, 5),
                            rotation,
                            Direction::West,
                            1,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size(south_piece),
                            rotation,
                            Mirror::None,
                        ));
                    }
                    if grid.get(x - 1, y) == 1 || (rooms.get(x - 1, y) & ROOM_CORRIDOR_FLAG) != 0 {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 1),
                            rotation,
                            Direction::North,
                            1,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size(west_piece),
                            rotation,
                            Mirror::None,
                        ));
                    }
                }
            }
        }

        // Interior walls, doors, rooms
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

                // Find door direction
                let mut door_dirs: Vec<Direction> = Vec::new();
                if (room_data & ROOM_DOOR_FLAG) != 0 {
                    for dir in &[
                        Direction::North,
                        Direction::East,
                        Direction::South,
                        Direction::West,
                    ] {
                        if grid.get(x + dir.offset().0, y + dir.offset().2) == 1 {
                            door_dirs.push(*dir);
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

                // West wall
                if is_house(grid, x - 1, y) && !mansion.is_room_id(x - 1, y, floor_num, room_id) {
                    let template = if door_dir == Some(Direction::West) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    pieces.push(piece_bb(
                        room_pos,
                        template_size(template),
                        rotation,
                        Mirror::None,
                    ));
                }

                // East wall (corridor side)
                if grid.get(x + 1, y) == 1 && !is_corridor_start {
                    let p = relative(room_pos, rotation, Direction::East, 8);
                    let template = if door_dir == Some(Direction::East) {
                        door_piece
                    } else {
                        wall_piece
                    };
                    pieces.push(piece_bb(p, template_size(template), rotation, Mirror::None));
                }

                // South wall
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
                    pieces.push(piece_bb(
                        p,
                        template_size(template),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                }

                // North wall (corridor side)
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
                    pieces.push(piece_bb(
                        p,
                        template_size(template),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                }

                // Room contents
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
                    if !mansion.is_room_id(
                        x + room_dir.offset().0,
                        y + room_dir.offset().2,
                        floor_num,
                        room_id,
                    ) {
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

fn place_entrance(pieces: &mut Vec<BoundingBox>, data: &mut PlacementData) {
    let pos = relative(data.position, data.rotation, Direction::West, 9);
    pieces.push(piece_bb(
        pos,
        template_size("entrance"),
        data.rotation,
        Mirror::None,
    ));
    data.position = relative(data.position, data.rotation, Direction::South, 16);
}

fn traverse_wall_piece(pieces: &mut Vec<BoundingBox>, data: &mut PlacementData) {
    let pos = relative(data.position, data.rotation, Direction::East, 7);
    pieces.push(piece_bb(
        pos,
        template_size(data.wall_type),
        data.rotation,
        Mirror::None,
    ));
    data.position = relative(data.position, data.rotation, Direction::South, 8);
}

fn traverse_turn(pieces: &mut Vec<BoundingBox>, data: &mut PlacementData) {
    data.position = relative(data.position, data.rotation, Direction::South, -1);
    pieces.push(piece_bb(
        data.position,
        template_size("wall_corner"),
        data.rotation,
        Mirror::None,
    ));
    data.position = relative(data.position, data.rotation, Direction::South, -7);
    data.position = relative(data.position, data.rotation, Direction::West, -6);
    data.rotation = compose_rotation(data.rotation, Rotation::Clockwise90);
}

const fn traverse_inner_turn(_pieces: &mut Vec<BoundingBox>, data: &mut PlacementData) {
    data.position = relative(data.position, data.rotation, Direction::South, 6);
    data.position = relative(data.position, data.rotation, Direction::East, 8);
    data.rotation = compose_rotation(data.rotation, Rotation::CounterClockwise90);
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla's traverseOuterWalls signature"
)]
fn traverse_outer_walls(
    pieces: &mut Vec<BoundingBox>,
    data: &mut PlacementData,
    grid: &SimpleGrid,
    initial_dir: Direction,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) {
    let mut grid_x = start_x;
    let mut grid_y = start_y;
    let mut dir = initial_dir;
    let start_dir = dir;

    loop {
        let (dx, dz) = (dir.offset().0, dir.offset().2);
        if !is_house(grid, grid_x + dx, grid_y + dz) {
            traverse_turn(pieces, data);
            dir = dir.rotate_y_clockwise();
            if grid_x != end_x || grid_y != end_y || start_dir != dir {
                traverse_wall_piece(pieces, data);
            }
        } else if is_house(grid, grid_x + dx, grid_y + dz)
            && is_house(
                grid,
                grid_x + dx + dir.rotate_y_counter_clockwise().offset().0,
                grid_y + dz + dir.rotate_y_counter_clockwise().offset().2,
            )
        {
            traverse_inner_turn(pieces, data);
            grid_x += dx;
            grid_y += dz;
            dir = dir.rotate_y_counter_clockwise();
        } else {
            grid_x += dx;
            grid_y += dz;
            if grid_x != end_x || grid_y != end_y || start_dir != dir {
                traverse_wall_piece(pieces, data);
            }
        }

        if grid_x == end_x && grid_y == end_y && start_dir == dir {
            break;
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "mirrors vanilla's MansionPiecePlacer.createRoof inline traversal"
)]
fn create_roof(
    pieces: &mut Vec<BoundingBox>,
    roof_origin: (i32, i32, i32),
    rotation: Rotation,
    grid: &SimpleGrid,
    above_grid: Option<&SimpleGrid>,
    start_x: i32,
    start_y: i32,
) {
    for y in 0..grid.height {
        for x in 0..grid.width {
            let mut pos = relative(
                roof_origin,
                rotation,
                Direction::South,
                8 + (y - start_y) * 8,
            );
            pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
            let is_above = above_grid.is_some_and(|g| is_house(g, x, y));

            if is_house(grid, x, y) && !is_above {
                pieces.push(piece_bb(
                    above(pos, 3),
                    template_size("roof"),
                    rotation,
                    Mirror::None,
                ));

                if !is_house(grid, x + 1, y) {
                    let p = relative(pos, rotation, Direction::East, 6);
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_front"),
                        rotation,
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x - 1, y) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 0),
                        rotation,
                        Direction::South,
                        7,
                    );
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_front"),
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y - 1) {
                    let p = relative(pos, rotation, Direction::West, 1);
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_front"),
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y + 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 6),
                        rotation,
                        Direction::South,
                        6,
                    );
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_front"),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                }
            }
        }
    }

    // Small walls between floors
    if let Some(above_g) = above_grid {
        for y in 0..grid.height {
            for x in 0..grid.width {
                let mut pos = relative(
                    roof_origin,
                    rotation,
                    Direction::South,
                    8 + (y - start_y) * 8,
                );
                pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
                let is_above = is_house(above_g, x, y);
                if !is_house(grid, x, y) || !is_above {
                    continue;
                }

                if !is_house(grid, x + 1, y) {
                    let p = relative(pos, rotation, Direction::East, 7);
                    pieces.push(piece_bb(
                        p,
                        template_size("small_wall"),
                        rotation,
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x - 1, y) {
                    let p = relative(
                        relative(pos, rotation, Direction::West, 1),
                        rotation,
                        Direction::South,
                        6,
                    );
                    pieces.push(piece_bb(
                        p,
                        template_size("small_wall"),
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y - 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::West, 0),
                        rotation,
                        Direction::North,
                        1,
                    );
                    pieces.push(piece_bb(
                        p,
                        template_size("small_wall"),
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y + 1) {
                    let p = relative(
                        relative(pos, rotation, Direction::East, 6),
                        rotation,
                        Direction::South,
                        7,
                    );
                    pieces.push(piece_bb(
                        p,
                        template_size("small_wall"),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                }

                // Corners
                if !is_house(grid, x + 1, y) {
                    if !is_house(grid, x, y - 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::East, 7),
                            rotation,
                            Direction::North,
                            2,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size("small_wall_corner"),
                            rotation,
                            Mirror::None,
                        ));
                    }
                    if !is_house(grid, x, y + 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::East, 8),
                            rotation,
                            Direction::South,
                            7,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size("small_wall_corner"),
                            compose_rotation(rotation, Rotation::Clockwise90),
                            Mirror::None,
                        ));
                    }
                }
                if !is_house(grid, x - 1, y) {
                    if !is_house(grid, x, y - 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 2),
                            rotation,
                            Direction::North,
                            1,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size("small_wall_corner"),
                            compose_rotation(rotation, Rotation::CounterClockwise90),
                            Mirror::None,
                        ));
                    }
                    if !is_house(grid, x, y + 1) {
                        let p = relative(
                            relative(pos, rotation, Direction::West, 1),
                            rotation,
                            Direction::South,
                            8,
                        );
                        pieces.push(piece_bb(
                            p,
                            template_size("small_wall_corner"),
                            compose_rotation(rotation, Rotation::Clockwise180),
                            Mirror::None,
                        ));
                    }
                }
            }
        }
    }

    // Roof corners and inner corners
    for y in 0..grid.height {
        for x in 0..grid.width {
            let mut pos = relative(
                roof_origin,
                rotation,
                Direction::South,
                8 + (y - start_y) * 8,
            );
            pos = relative(pos, rotation, Direction::East, (x - start_x) * 8);
            let is_above = above_grid.is_some_and(|g| is_house(g, x, y));
            if !is_house(grid, x, y) || is_above {
                continue;
            }

            // East side corners
            if !is_house(grid, x + 1, y) {
                let p = relative(pos, rotation, Direction::East, 6);
                if !is_house(grid, x, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 6);
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_corner"),
                        rotation,
                        Mirror::None,
                    ));
                } else if is_house(grid, x + 1, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 5);
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_inner_corner"),
                        rotation,
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y - 1) {
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_corner"),
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    ));
                } else if is_house(grid, x + 1, y - 1) {
                    let p2 = relative(
                        relative(pos, rotation, Direction::East, 9),
                        rotation,
                        Direction::North,
                        2,
                    );
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_inner_corner"),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                }
            }

            // West side corners
            if !is_house(grid, x - 1, y) {
                let p = relative(pos, rotation, Direction::East, 0);
                let p = relative(p, rotation, Direction::South, 0);
                if !is_house(grid, x, y + 1) {
                    let p2 = relative(p, rotation, Direction::South, 6);
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_corner"),
                        compose_rotation(rotation, Rotation::Clockwise90),
                        Mirror::None,
                    ));
                } else if is_house(grid, x - 1, y + 1) {
                    let p2 = relative(
                        relative(p, rotation, Direction::South, 8),
                        rotation,
                        Direction::West,
                        3,
                    );
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_inner_corner"),
                        compose_rotation(rotation, Rotation::CounterClockwise90),
                        Mirror::None,
                    ));
                }
                if !is_house(grid, x, y - 1) {
                    pieces.push(piece_bb(
                        p,
                        template_size("roof_corner"),
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    ));
                } else if is_house(grid, x - 1, y - 1) {
                    let p2 = relative(p, rotation, Direction::South, 1);
                    pieces.push(piece_bb(
                        p2,
                        template_size("roof_inner_corner"),
                        compose_rotation(rotation, Rotation::Clockwise180),
                        Mirror::None,
                    ));
                }
            }
        }
    }
}

fn add_room_1x1(
    pieces: &mut Vec<BoundingBox>,
    room_pos: (i32, i32, i32),
    rotation: Rotation,
    door_dir: Option<Direction>,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let mut piece_rot = Rotation::None;
    let kind;
    match door_dir {
        Some(Direction::East) => kind = "1x1",
        Some(Direction::North) => {
            piece_rot = Rotation::CounterClockwise90;
            kind = "1x1";
        }
        Some(Direction::West) => {
            piece_rot = Rotation::Clockwise180;
            kind = "1x1";
        }
        Some(Direction::South) => {
            piece_rot = Rotation::Clockwise90;
            kind = "1x1";
        }
        _ => kind = "1x1s",
    }
    let name = get_room_name(rng, floor, kind, false);
    let orient = zero_pos_transform((1, 0, 0), piece_rot, 7, 7);
    piece_rot = compose_rotation(piece_rot, rotation);
    let orient = rotation.transform_pos(orient.0, orient.1, orient.2, 0, 0);
    let pos = (room_pos.0 + orient.0, room_pos.1, room_pos.2 + orient.2);
    pieces.push(piece_bb(pos, template_size(&name), piece_rot, Mirror::None));
}

#[expect(
    clippy::too_many_arguments,
    reason = "mirrors vanilla's MansionRoom1x2 constructor surface"
)]
#[expect(
    clippy::too_many_lines,
    reason = "exhaustive (door_dir × room_dir) dispatch mirroring vanilla's MansionRoom1x2"
)]
fn add_room_1x2(
    pieces: &mut Vec<BoundingBox>,
    room_pos: (i32, i32, i32),
    rotation: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    is_stairs: bool,
    rng: &mut LegacyRandom,
) {
    let (pos, rot, mirror, kind) = match (door_dir, room_dir) {
        (Direction::East, Direction::South) => (
            relative(room_pos, rotation, Direction::East, 1),
            rotation,
            Mirror::None,
            "1x2side",
        ),
        (Direction::East, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::South,
                6,
            );
            (p, rotation, Mirror::LeftRight, "1x2side")
        }
        (Direction::West, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise180),
                Mirror::None,
                "1x2side",
            )
        }
        (Direction::West, Direction::South) => (
            relative(room_pos, rotation, Direction::East, 7),
            rotation,
            Mirror::FrontBack,
            "1x2side",
        ),
        (Direction::South, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 1),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::LeftRight,
            "1x2side",
        ),
        (Direction::South, Direction::West) => (
            relative(room_pos, rotation, Direction::East, 7),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2side",
        ),
        (Direction::North, Direction::West) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise90),
                Mirror::FrontBack,
                "1x2side",
            )
        }
        (Direction::North, Direction::East) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::CounterClockwise90),
                Mirror::None,
                "1x2side",
            )
        }
        (Direction::South, Direction::North) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::North,
                8,
            );
            (p, rotation, Mirror::None, "1x2front")
        }
        (Direction::North, Direction::South) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 7),
                rotation,
                Direction::South,
                14,
            );
            (
                p,
                compose_rotation(rotation, Rotation::Clockwise180),
                Mirror::None,
                "1x2front",
            )
        }
        (Direction::West, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 15),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2front",
        ),
        (Direction::East, Direction::West) => {
            let p = relative(
                relative(room_pos, rotation, Direction::West, 7),
                rotation,
                Direction::South,
                6,
            );
            (
                p,
                compose_rotation(rotation, Rotation::CounterClockwise90),
                Mirror::None,
                "1x2front",
            )
        }
        (Direction::Up, Direction::East) => (
            relative(room_pos, rotation, Direction::East, 15),
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
            "1x2secret",
        ),
        (Direction::Up, Direction::South) => {
            let p = relative(
                relative(room_pos, rotation, Direction::East, 1),
                rotation,
                Direction::North,
                0,
            );
            (p, rotation, Mirror::None, "1x2secret")
        }
        _ => return,
    };

    let name = get_room_name(rng, floor, kind, is_stairs);
    pieces.push(piece_bb(pos, template_size(&name), rot, mirror));
}

fn add_room_2x2(
    pieces: &mut Vec<BoundingBox>,
    room_pos: (i32, i32, i32),
    rotation: Rotation,
    room_dir: Direction,
    door_dir: Direction,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let (east, south, rot, mirror) = match (door_dir, room_dir) {
        (Direction::East, Direction::South) => (-7, 0, rotation, Mirror::None),
        (Direction::East, Direction::North) => (-7, 6, rotation, Mirror::LeftRight),
        (Direction::North, Direction::East) => (
            1,
            14,
            compose_rotation(rotation, Rotation::CounterClockwise90),
            Mirror::None,
        ),
        (Direction::North, Direction::West) => (
            7,
            14,
            compose_rotation(rotation, Rotation::CounterClockwise90),
            Mirror::LeftRight,
        ),
        (Direction::South, Direction::West) => (
            7,
            -8,
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::None,
        ),
        (Direction::South, Direction::East) => (
            1,
            -8,
            compose_rotation(rotation, Rotation::Clockwise90),
            Mirror::LeftRight,
        ),
        (Direction::West, Direction::North) => (
            15,
            6,
            compose_rotation(rotation, Rotation::Clockwise180),
            Mirror::None,
        ),
        (Direction::West, Direction::South) => (15, 0, rotation, Mirror::FrontBack),
        _ => return,
    };

    let pos = relative(
        relative(room_pos, rotation, Direction::East, east),
        rotation,
        Direction::South,
        south,
    );
    let name = get_room_name(rng, floor, "2x2", false);
    pieces.push(piece_bb(pos, template_size(&name), rot, mirror));
}

fn add_room_2x2_secret(
    pieces: &mut Vec<BoundingBox>,
    room_pos: (i32, i32, i32),
    rotation: Rotation,
    floor: usize,
    rng: &mut LegacyRandom,
) {
    let pos = relative(room_pos, rotation, Direction::East, 1);
    let name = get_room_name(rng, floor, "2x2secret", false);
    pieces.push(piece_bb(pos, template_size(&name), rotation, Mirror::None));
}

/// `Structure` impl — registered under `"minecraft:woodland_mansion"`.
///
/// Vanilla's `WoodlandMansionStructure.findGenerationPoint`: consumes a
/// rotation, probes a rotation-dependent 5×5 box for the lowest Y, rejects
/// if `< 60`, then runs `generate_mansion_pieces`.
pub struct WoodlandMansionStructure;

impl Structure for WoodlandMansionStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let rotation = Rotation::get_random(rng);

        let (off_x, off_z) = match rotation {
            Rotation::None => (5, 5),
            Rotation::Clockwise90 => (-5, 5),
            Rotation::Clockwise180 => (-5, -5),
            Rotation::CounterClockwise90 => (5, -5),
        };
        let bx = ctx.chunk_min_x() + 7;
        let bz = ctx.chunk_min_z() + 7;
        let h0 = ctx.base_height(bx, bz, false);
        let h1 = ctx.base_height(bx, bz + off_z, false);
        let h2 = ctx.base_height(bx + off_x, bz, false);
        let h3 = ctx.base_height(bx + off_x, bz + off_z, false);
        let lowest = h0.min(h1).min(h2).min(h3);
        if lowest < 60 {
            return None;
        }

        let biome = ctx.biome_at(bx, lowest, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let bbs = generate_mansion_pieces((bx, lowest, bz), rotation, rng);
        let pieces = bbs
            .into_iter()
            .map(|bb| {
                StructurePiece::non_jigsaw(
                    Identifier::new_static("minecraft", "wmp"),
                    bb,
                    0,
                    Some(Direction::North),
                )
            })
            .collect();

        Some(GenerationStub {
            position: (bx, lowest, bz),
            pieces,
        })
    }
}
