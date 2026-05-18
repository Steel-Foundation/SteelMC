//! Ocean monument structure start generation and runtime placement state.
//!
//! Special biome check: every biome in a 29-block (3D) radius around
//! `(chunkMinX+9, seaLevel, chunkMinZ+9)` must be in `#required_ocean_monument_surrounding`.

use steel_registry::structure::StructureData;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BlockPos, BoundingBox, Direction, Identifier};

use crate::world::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload, make_oriented_piece_bounding_box, random_horizontal_direction,
};

const WIDTH: i32 = 58;
const HEIGHT: i32 = 23;
const DEPTH: i32 = 58;
const MIN_Y: i32 = 39;
const BIOME_RANGE_CHECK: i32 = 29;
const GRID_WIDTH: i32 = 5;
const GRID_DEPTH: i32 = 5;
const GRID_FLOOR_COUNT: i32 = GRID_WIDTH * GRID_DEPTH;
const GRID_SIZE: usize = 75;
const GRIDROOM_SOURCE_INDEX: i32 = get_room_index(2, 0, 0);
const GRIDROOM_TOP_CONNECT_INDEX: i32 = get_room_index(2, 2, 0);
const GRIDROOM_LEFTWING_CONNECT_INDEX: i32 = get_room_index(0, 1, 0);
const GRIDROOM_RIGHTWING_CONNECT_INDEX: i32 = get_room_index(4, 1, 0);
const LEFTWING_INDEX: i32 = 1001;
const RIGHTWING_INDEX: i32 = 1002;
const PENTHOUSE_INDEX: i32 = 1003;

/// `#minecraft:required_ocean_monument_surrounding`.
const SURROUNDING_BIOMES: &[&str] = &[
    "deep_frozen_ocean",
    "deep_cold_ocean",
    "deep_ocean",
    "deep_lukewarm_ocean",
    "frozen_ocean",
    "cold_ocean",
    "ocean",
    "lukewarm_ocean",
    "warm_ocean",
    "river",
    "frozen_river",
];

/// Runtime state for vanilla `OceanMonumentPieces.MonumentBuilding`.
#[derive(Debug, Clone, PartialEq)]
pub struct OceanMonumentPieceData {
    /// Internal child pieces generated and placed by `MonumentBuilding`.
    pub child_pieces: Vec<OceanMonumentChildPiece>,
}

/// One internal ocean-monument child piece.
#[derive(Debug, Clone, PartialEq)]
pub struct OceanMonumentChildPiece {
    /// World-space child bounding box after vanilla's building-relative offset.
    pub bounding_box: BoundingBox,
    /// Child piece variant and variant-specific persisted state.
    pub kind: OceanMonumentChildPieceKind,
}

/// Variant-specific data for monument child pieces.
#[derive(Debug, Clone, PartialEq)]
pub enum OceanMonumentChildPieceKind {
    /// `OceanMonumentEntryRoom`.
    EntryRoom {
        /// Source room snapshot.
        room: OceanMonumentRoomData,
    },
    /// `OceanMonumentCoreRoom`.
    CoreRoom,
    /// `OceanMonumentDoubleXRoom`.
    DoubleXRoom {
        /// Western room snapshot.
        west: OceanMonumentRoomData,
        /// Eastern room snapshot.
        east: OceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleXYRoom`.
    DoubleXYRoom {
        /// Lower western room.
        west: OceanMonumentRoomData,
        /// Lower eastern room.
        east: OceanMonumentRoomData,
        /// Upper western room.
        west_up: OceanMonumentRoomData,
        /// Upper eastern room.
        east_up: OceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleYRoom`.
    DoubleYRoom {
        /// Lower room.
        room: OceanMonumentRoomData,
        /// Upper room.
        above: OceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleYZRoom`.
    DoubleYZRoom {
        /// Southern lower room.
        south: OceanMonumentRoomData,
        /// Northern lower room.
        north: OceanMonumentRoomData,
        /// Southern upper room.
        south_up: OceanMonumentRoomData,
        /// Northern upper room.
        north_up: OceanMonumentRoomData,
    },
    /// `OceanMonumentDoubleZRoom`.
    DoubleZRoom {
        /// Southern room.
        south: OceanMonumentRoomData,
        /// Northern room.
        north: OceanMonumentRoomData,
    },
    /// `OceanMonumentSimpleRoom`.
    SimpleRoom {
        /// Room snapshot.
        room: OceanMonumentRoomData,
        /// Vanilla `mainDesign`.
        main_design: i32,
    },
    /// `OceanMonumentSimpleTopRoom`.
    SimpleTopRoom {
        /// Room snapshot.
        room: OceanMonumentRoomData,
    },
    /// `OceanMonumentWingRoom`.
    WingRoom {
        /// Vanilla `mainDesign`.
        main_design: i32,
    },
    /// `OceanMonumentPenthouse`.
    Penthouse,
}

/// Persisted room state consumed by child placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OceanMonumentRoomData {
    /// Vanilla room index.
    pub index: i32,
    /// Vanilla `hasOpening`, ordered by `Direction.get3DDataValue`.
    pub has_opening: [bool; 6],
    /// Whether `connections[UP] != null`; distinct from an open upward passage.
    pub has_up_connection: bool,
}

impl OceanMonumentRoomData {
    /// Counts open sides exactly like vanilla `RoomDefinition.countOpenings`.
    #[must_use]
    pub fn count_openings(self) -> i32 {
        self.has_opening.iter().filter(|&&open| open).count() as i32
    }
}

/// Registered under `"minecraft:ocean_monument"`.
pub struct OceanMonumentStructure;

impl Structure for OceanMonumentStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let check_x = ctx.chunk_min_x() + 9;
        let check_z = ctx.chunk_min_z() + 9;
        let check_y = ctx.sea_level();

        let x_range = ((check_x - BIOME_RANGE_CHECK) >> 2)..=((check_x + BIOME_RANGE_CHECK) >> 2);
        let z_range = ((check_z - BIOME_RANGE_CHECK) >> 2)..=((check_z + BIOME_RANGE_CHECK) >> 2);
        let y_range = ((check_y - BIOME_RANGE_CHECK) >> 2)..=((check_y + BIOME_RANGE_CHECK) >> 2);

        for qz in z_range {
            for qx in x_range.clone() {
                for qy in y_range.clone() {
                    let biome = ctx.biome_at(qx << 2, qy << 2, qz << 2);
                    if !SURROUNDING_BIOMES
                        .iter()
                        .any(|&b| biome.key == Identifier::vanilla_static(b))
                    {
                        return None;
                    }
                }
            }
        }

        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let west = ctx.chunk_min_x() - 29;
        let north = ctx.chunk_min_z() - 29;
        let orientation = random_horizontal_direction(rng);
        let piece = monument_building_piece(west, north, orientation, rng);
        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: vec![piece],
        })
    }
}

fn monument_building_piece(
    west: i32,
    north: i32,
    orientation: Direction,
    rng: &mut LegacyRandom,
) -> StructurePiece {
    let bounding_box =
        make_oriented_piece_bounding_box(west, MIN_Y, north, orientation, WIDTH, HEIGHT, DEPTH);
    StructurePiece {
        piece_type: Identifier::new_static("minecraft", "omb"),
        bounding_box,
        gen_depth: 0,
        orientation: Some(orientation),
        payload: StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(
            OceanMonumentPieceData {
                child_pieces: generate_child_pieces(bounding_box, orientation, rng),
            },
        )),
        ground_level_delta: 0,
        junctions: Vec::new(),
        projection: None,
    }
}

fn generate_child_pieces(
    building_box: BoundingBox,
    orientation: Direction,
    rng: &mut LegacyRandom,
) -> Vec<OceanMonumentChildPiece> {
    let mut graph = generate_room_graph(rng);
    let mut child_pieces = Vec::new();

    graph.rooms[graph.source_room].claimed = true;
    push_room_child(
        &mut child_pieces,
        OceanMonumentChildPieceKind::EntryRoom {
            room: graph.snapshot(graph.source_room),
        },
        orientation,
        graph.rooms[graph.source_room].index,
        (1, 1, 1),
    );
    push_room_child(
        &mut child_pieces,
        OceanMonumentChildPieceKind::CoreRoom,
        orientation,
        graph.rooms[graph.core_room].index,
        (2, 2, 2),
    );

    for order_index in 0..graph.room_order.len() {
        let room_index = graph.room_order[order_index];
        if graph.rooms[room_index].claimed || graph.rooms[room_index].is_special() {
            continue;
        }

        if fit_double_xy_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        if fit_double_yz_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        if fit_double_z_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        if fit_double_x_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        if fit_double_y_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        if fit_simple_top_room(&mut graph, &mut child_pieces, orientation, room_index) {
            continue;
        }
        fit_simple_room(&mut graph, &mut child_pieces, orientation, room_index, rng);
    }

    let offset = world_pos(building_box, Some(orientation), 9, 0, 22);
    for child in &mut child_pieces {
        child.bounding_box = child.bounding_box.moved(offset.x(), offset.y(), offset.z());
    }

    let left_wing = BoundingBox::from_corners(
        world_pos(building_box, Some(orientation), 1, 1, 1),
        world_pos(building_box, Some(orientation), 23, 8, 21),
    );
    let right_wing = BoundingBox::from_corners(
        world_pos(building_box, Some(orientation), 34, 1, 1),
        world_pos(building_box, Some(orientation), 56, 8, 21),
    );
    let penthouse = BoundingBox::from_corners(
        world_pos(building_box, Some(orientation), 22, 13, 22),
        world_pos(building_box, Some(orientation), 35, 17, 35),
    );
    let wing_random = rng.next_i32();
    child_pieces.push(OceanMonumentChildPiece {
        bounding_box: left_wing,
        kind: OceanMonumentChildPieceKind::WingRoom {
            main_design: wing_random & 1,
        },
    });
    child_pieces.push(OceanMonumentChildPiece {
        bounding_box: right_wing,
        kind: OceanMonumentChildPieceKind::WingRoom {
            main_design: wing_random.wrapping_add(1) & 1,
        },
    });
    child_pieces.push(OceanMonumentChildPiece {
        bounding_box: penthouse,
        kind: OceanMonumentChildPieceKind::Penthouse,
    });

    child_pieces
}

struct RoomGraph {
    rooms: Vec<RoomDefinition>,
    room_order: Vec<usize>,
    source_room: usize,
    core_room: usize,
}

impl RoomGraph {
    fn snapshot(&self, room: usize) -> OceanMonumentRoomData {
        let definition = &self.rooms[room];
        OceanMonumentRoomData {
            index: definition.index,
            has_opening: definition.has_opening,
            has_up_connection: definition.connections[direction_index(Direction::Up)].is_some(),
        }
    }

    fn connection(&self, room: usize, direction: Direction) -> usize {
        let Some(connection) = self.rooms[room].connections[direction_index(direction)] else {
            panic!(
                "ocean monument room {} missing {:?} connection",
                self.rooms[room].index, direction
            );
        };
        connection
    }
}

#[derive(Debug, Clone)]
struct RoomDefinition {
    index: i32,
    connections: [Option<usize>; 6],
    has_opening: [bool; 6],
    claimed: bool,
    is_source: bool,
    scan_index: i32,
}

impl RoomDefinition {
    const fn new(index: i32) -> Self {
        Self {
            index,
            connections: [None; 6],
            has_opening: [false; 6],
            claimed: false,
            is_source: false,
            scan_index: 0,
        }
    }

    const fn is_special(&self) -> bool {
        self.index >= GRID_SIZE as i32
    }

    fn update_openings(&mut self) {
        for i in 0..6 {
            self.has_opening[i] = self.connections[i].is_some();
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "direct port of vanilla MonumentBuilding.generateRoomGraph"
)]
fn generate_room_graph(rng: &mut LegacyRandom) -> RoomGraph {
    let mut rooms = Vec::new();
    let mut room_grid = [None; GRID_SIZE];

    for x in 0..5 {
        for z in 0..4 {
            let pos = get_room_index(x, 0, z);
            room_grid[pos as usize] = Some(add_room(&mut rooms, pos));
        }
    }

    for x in 0..5 {
        for z in 0..4 {
            let pos = get_room_index(x, 1, z);
            room_grid[pos as usize] = Some(add_room(&mut rooms, pos));
        }
    }

    for x in 1..4 {
        for z in 0..2 {
            let pos = get_room_index(x, 2, z);
            room_grid[pos as usize] = Some(add_room(&mut rooms, pos));
        }
    }

    let source_room = room_at(&room_grid, GRIDROOM_SOURCE_INDEX);

    for x in 0..5 {
        for z in 0..5 {
            for y in 0..3 {
                let pos = get_room_index(x, y, z);
                let Some(room_index) = room_grid[pos as usize] else {
                    continue;
                };

                for direction in VANILLA_DIRECTIONS {
                    let (step_x, step_y, step_z) = direction.offset();
                    let neigh_x = x + step_x;
                    let neigh_y = y + step_y;
                    let neigh_z = z + step_z;
                    if !(0..5).contains(&neigh_x)
                        || !(0..5).contains(&neigh_z)
                        || !(0..3).contains(&neigh_y)
                    {
                        continue;
                    }

                    let neigh_pos = get_room_index(neigh_x, neigh_y, neigh_z);
                    let Some(neighbor) = room_grid[neigh_pos as usize] else {
                        continue;
                    };
                    let connection_direction = if neigh_z == z {
                        direction
                    } else {
                        direction.opposite()
                    };
                    set_connection(&mut rooms, room_index, connection_direction, neighbor);
                }
            }
        }
    }

    let roof_room = add_room(&mut rooms, PENTHOUSE_INDEX);
    let left_wing = add_room(&mut rooms, LEFTWING_INDEX);
    let right_wing = add_room(&mut rooms, RIGHTWING_INDEX);
    set_connection(
        &mut rooms,
        room_at(&room_grid, GRIDROOM_TOP_CONNECT_INDEX),
        Direction::Up,
        roof_room,
    );
    set_connection(
        &mut rooms,
        room_at(&room_grid, GRIDROOM_LEFTWING_CONNECT_INDEX),
        Direction::South,
        left_wing,
    );
    set_connection(
        &mut rooms,
        room_at(&room_grid, GRIDROOM_RIGHTWING_CONNECT_INDEX),
        Direction::South,
        right_wing,
    );
    rooms[roof_room].claimed = true;
    rooms[left_wing].claimed = true;
    rooms[right_wing].claimed = true;
    rooms[source_room].is_source = true;

    let core_room = room_at(&room_grid, get_room_index(rng.next_i32_bounded(4), 0, 2));
    rooms[core_room].claimed = true;
    let core_east = connected_room(&rooms, core_room, Direction::East);
    let core_north = connected_room(&rooms, core_room, Direction::North);
    let core_east_north = connected_room(&rooms, core_east, Direction::North);
    let core_up = connected_room(&rooms, core_room, Direction::Up);
    let core_east_up = connected_room(&rooms, core_east, Direction::Up);
    let core_north_up = connected_room(&rooms, core_north, Direction::Up);
    let core_east_north_up = connected_room(&rooms, core_east_north, Direction::Up);
    for room in [
        core_east,
        core_north,
        core_east_north,
        core_up,
        core_east_up,
        core_north_up,
        core_east_north_up,
    ] {
        rooms[room].claimed = true;
    }

    let mut room_order = Vec::new();
    for room in room_grid.into_iter().flatten() {
        rooms[room].update_openings();
        room_order.push(room);
    }
    rooms[roof_room].update_openings();
    vanilla_shuffle(&mut room_order, rng);

    let mut scan_index = 1;
    for room in room_order.iter().copied() {
        let mut close_count = 0;
        let mut attempt_count = 0;

        while close_count < 2 && attempt_count < 5 {
            attempt_count += 1;
            let direction = rng.next_i32_bounded(6) as usize;
            if !rooms[room].has_opening[direction] {
                continue;
            }

            let Some(connection) = rooms[room].connections[direction] else {
                continue;
            };
            let opposite = direction_index(direction_from_index(direction).opposite());
            rooms[room].has_opening[direction] = false;
            rooms[connection].has_opening[opposite] = false;

            let room_scan = scan_index;
            scan_index += 1;
            let connection_scan = scan_index;
            scan_index += 1;
            if find_source(&mut rooms, room, room_scan)
                && find_source(&mut rooms, connection, connection_scan)
            {
                close_count += 1;
            } else {
                rooms[room].has_opening[direction] = true;
                rooms[connection].has_opening[opposite] = true;
            }
        }
    }

    room_order.push(roof_room);
    room_order.push(left_wing);
    room_order.push(right_wing);

    RoomGraph {
        rooms,
        room_order,
        source_room,
        core_room,
    }
}

fn add_room(rooms: &mut Vec<RoomDefinition>, index: i32) -> usize {
    let room = rooms.len();
    rooms.push(RoomDefinition::new(index));
    room
}

fn room_at(room_grid: &[Option<usize>; GRID_SIZE], room_index: i32) -> usize {
    let Some(room) = room_grid[room_index as usize] else {
        panic!("ocean monument missing generated room {room_index}");
    };
    room
}

fn connected_room(rooms: &[RoomDefinition], room: usize, direction: Direction) -> usize {
    let Some(connection) = rooms[room].connections[direction_index(direction)] else {
        panic!(
            "ocean monument room {} missing {:?} connection",
            rooms[room].index, direction
        );
    };
    connection
}

fn set_connection(
    rooms: &mut [RoomDefinition],
    room: usize,
    direction: Direction,
    connection: usize,
) {
    rooms[room].connections[direction_index(direction)] = Some(connection);
    rooms[connection].connections[direction_index(direction.opposite())] = Some(room);
}

fn find_source(rooms: &mut [RoomDefinition], start: usize, scan_index: i32) -> bool {
    let mut stack = vec![start];
    while let Some(room) = stack.pop() {
        if rooms[room].is_source {
            return true;
        }
        if rooms[room].scan_index == scan_index {
            continue;
        }
        rooms[room].scan_index = scan_index;

        for direction in 0..6 {
            let Some(connection) = rooms[room].connections[direction] else {
                continue;
            };
            if rooms[room].has_opening[direction] && rooms[connection].scan_index != scan_index {
                stack.push(connection);
            }
        }
    }
    false
}

fn fit_double_xy_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    if !graph.rooms[room].has_opening[direction_index(Direction::East)]
        || !graph.rooms[room].has_opening[direction_index(Direction::Up)]
    {
        return false;
    }

    let east = graph.connection(room, Direction::East);
    let up = graph.connection(room, Direction::Up);
    if graph.rooms[east].claimed || graph.rooms[up].claimed {
        return false;
    }

    if !graph.rooms[east].has_opening[direction_index(Direction::Up)] {
        return false;
    }
    let east_up = graph.connection(east, Direction::Up);
    if graph.rooms[east_up].claimed {
        return false;
    }

    for claimed in [room, east, up, east_up] {
        graph.rooms[claimed].claimed = true;
    }
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::DoubleXYRoom {
            west: graph.snapshot(room),
            east: graph.snapshot(east),
            west_up: graph.snapshot(up),
            east_up: graph.snapshot(east_up),
        },
        orientation,
        graph.rooms[room].index,
        (2, 2, 1),
    );
    true
}

fn fit_double_yz_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    if !graph.rooms[room].has_opening[direction_index(Direction::North)]
        || !graph.rooms[room].has_opening[direction_index(Direction::Up)]
    {
        return false;
    }

    let north = graph.connection(room, Direction::North);
    let up = graph.connection(room, Direction::Up);
    if graph.rooms[north].claimed || graph.rooms[up].claimed {
        return false;
    }

    if !graph.rooms[north].has_opening[direction_index(Direction::Up)] {
        return false;
    }
    let north_up = graph.connection(north, Direction::Up);
    if graph.rooms[north_up].claimed {
        return false;
    }

    for claimed in [room, north, up, north_up] {
        graph.rooms[claimed].claimed = true;
    }
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::DoubleYZRoom {
            south: graph.snapshot(room),
            north: graph.snapshot(north),
            south_up: graph.snapshot(up),
            north_up: graph.snapshot(north_up),
        },
        orientation,
        graph.rooms[room].index,
        (1, 2, 2),
    );
    true
}

fn fit_double_z_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    if !graph.rooms[room].has_opening[direction_index(Direction::North)]
        || graph.rooms[graph.connection(room, Direction::North)].claimed
    {
        return false;
    }

    let north = graph.connection(room, Direction::North);
    graph.rooms[room].claimed = true;
    graph.rooms[north].claimed = true;
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::DoubleZRoom {
            south: graph.snapshot(room),
            north: graph.snapshot(north),
        },
        orientation,
        graph.rooms[room].index,
        (1, 1, 2),
    );
    true
}

fn fit_double_x_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    if !graph.rooms[room].has_opening[direction_index(Direction::East)] {
        return false;
    }

    let east = graph.connection(room, Direction::East);
    if graph.rooms[east].claimed {
        return false;
    }

    graph.rooms[room].claimed = true;
    graph.rooms[east].claimed = true;
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::DoubleXRoom {
            west: graph.snapshot(room),
            east: graph.snapshot(east),
        },
        orientation,
        graph.rooms[room].index,
        (2, 1, 1),
    );
    true
}

fn fit_double_y_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    if !graph.rooms[room].has_opening[direction_index(Direction::Up)] {
        return false;
    }

    let above = graph.connection(room, Direction::Up);
    if graph.rooms[above].claimed {
        return false;
    }

    graph.rooms[room].claimed = true;
    graph.rooms[above].claimed = true;
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::DoubleYRoom {
            room: graph.snapshot(room),
            above: graph.snapshot(above),
        },
        orientation,
        graph.rooms[room].index,
        (1, 2, 1),
    );
    true
}

fn fit_simple_top_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
) -> bool {
    let definition = &graph.rooms[room];
    if definition.has_opening[direction_index(Direction::West)]
        || definition.has_opening[direction_index(Direction::East)]
        || definition.has_opening[direction_index(Direction::North)]
        || definition.has_opening[direction_index(Direction::South)]
        || definition.has_opening[direction_index(Direction::Up)]
    {
        return false;
    }

    graph.rooms[room].claimed = true;
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::SimpleTopRoom {
            room: graph.snapshot(room),
        },
        orientation,
        graph.rooms[room].index,
        (1, 1, 1),
    );
    true
}

fn fit_simple_room(
    graph: &mut RoomGraph,
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    orientation: Direction,
    room: usize,
    rng: &mut LegacyRandom,
) {
    graph.rooms[room].claimed = true;
    push_room_child(
        child_pieces,
        OceanMonumentChildPieceKind::SimpleRoom {
            room: graph.snapshot(room),
            main_design: rng.next_i32_bounded(3),
        },
        orientation,
        graph.rooms[room].index,
        (1, 1, 1),
    );
}

fn push_room_child(
    child_pieces: &mut Vec<OceanMonumentChildPiece>,
    kind: OceanMonumentChildPieceKind,
    orientation: Direction,
    room_index: i32,
    size: (i32, i32, i32),
) {
    child_pieces.push(OceanMonumentChildPiece {
        bounding_box: make_room_bounding_box(orientation, room_index, size.0, size.1, size.2),
        kind,
    });
}

fn make_room_bounding_box(
    orientation: Direction,
    room_index: i32,
    room_width: i32,
    room_height: i32,
    room_depth: i32,
) -> BoundingBox {
    let room_x = room_index % GRID_WIDTH;
    let room_z = room_index / GRID_WIDTH % GRID_DEPTH;
    let room_y = room_index / GRID_FLOOR_COUNT;
    let bounding_box = make_oriented_piece_bounding_box(
        0,
        0,
        0,
        orientation,
        room_width * 8,
        room_height * 4,
        room_depth * 8,
    );

    match orientation {
        Direction::North => {
            bounding_box.moved(room_x * 8, room_y * 4, -(room_z + room_depth) * 8 + 1)
        }
        Direction::South => bounding_box.moved(room_x * 8, room_y * 4, room_z * 8),
        Direction::West => {
            bounding_box.moved(-(room_z + room_depth) * 8 + 1, room_y * 4, room_x * 8)
        }
        Direction::East => bounding_box.moved(room_z * 8, room_y * 4, room_x * 8),
        Direction::Down | Direction::Up => panic!("ocean monument room has vertical orientation"),
    }
}

const fn world_pos(
    bounding_box: BoundingBox,
    orientation: Option<Direction>,
    x: i32,
    y: i32,
    z: i32,
) -> BlockPos {
    let world_y = if orientation.is_some() {
        y + bounding_box.min_y
    } else {
        y
    };
    let (world_x, world_z) = match orientation {
        None | Some(Direction::Up | Direction::Down) => (x, z),
        Some(Direction::North) => (bounding_box.min_x + x, bounding_box.max_z - z),
        Some(Direction::South) => (bounding_box.min_x + x, bounding_box.min_z + z),
        Some(Direction::West) => (bounding_box.max_x - z, bounding_box.min_z + x),
        Some(Direction::East) => (bounding_box.min_x + z, bounding_box.min_z + x),
    };
    BlockPos::new(world_x, world_y, world_z)
}

const VANILLA_DIRECTIONS: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

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

const fn direction_from_index(index: usize) -> Direction {
    match index {
        0 => Direction::Down,
        1 => Direction::Up,
        2 => Direction::North,
        3 => Direction::South,
        4 => Direction::West,
        _ => Direction::East,
    }
}

const fn get_room_index(room_x: i32, room_y: i32, room_z: i32) -> i32 {
    room_y * GRID_FLOOR_COUNT + room_z * GRID_WIDTH + room_x
}

fn vanilla_shuffle<T>(items: &mut [T], rng: &mut LegacyRandom) {
    for i in (1..items.len()).rev() {
        let j = rng.next_i32_bounded((i + 1) as i32) as usize;
        items.swap(i, j);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monument_building_uses_full_procedural_payload() {
        let mut rng = LegacyRandom::from_seed(1234);
        let piece = monument_building_piece(16, 32, Direction::West, &mut rng);

        assert_eq!(piece.piece_type, Identifier::new_static("minecraft", "omb"));
        assert_eq!(piece.gen_depth, 0);
        assert_eq!(piece.orientation, Some(Direction::West));
        assert_eq!(piece.bounding_box, BoundingBox::new(16, 39, 32, 73, 61, 89));
        let StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(data)) =
            piece.payload
        else {
            panic!("ocean monument should use its procedural payload");
        };
        assert!(!data.child_pieces.is_empty());
        assert!(matches!(
            data.child_pieces[0].kind,
            OceanMonumentChildPieceKind::EntryRoom { .. }
        ));
        assert!(matches!(
            data.child_pieces[1].kind,
            OceanMonumentChildPieceKind::CoreRoom
        ));
        assert!(matches!(
            data.child_pieces.last().expect("penthouse child").kind,
            OceanMonumentChildPieceKind::Penthouse
        ));
    }

    #[test]
    fn generated_child_order_captures_vanilla_fixed_children() {
        let mut rng = LegacyRandom::from_seed(9876);
        let piece = monument_building_piece(-29, -29, Direction::South, &mut rng);
        let StructurePiecePayload::Procedural(ProceduralPieceData::OceanMonument(data)) =
            piece.payload
        else {
            panic!("ocean monument should use its procedural payload");
        };

        assert!(data.child_pieces.len() >= 5);
        assert!(matches!(
            data.child_pieces[data.child_pieces.len() - 3].kind,
            OceanMonumentChildPieceKind::WingRoom { .. }
        ));
        assert!(matches!(
            data.child_pieces[data.child_pieces.len() - 2].kind,
            OceanMonumentChildPieceKind::WingRoom { .. }
        ));
        assert!(matches!(
            data.child_pieces[data.child_pieces.len() - 1].kind,
            OceanMonumentChildPieceKind::Penthouse
        ));
    }
}
