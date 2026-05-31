//! Mineshaft. Vanilla's `MineshaftPieces` DFS: each child's `addChildren` runs
//! immediately after creation, before processing the next sibling.

use steel_registry::structure::{MineshaftTypeData, StructureConfigData, StructureData};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload,
};

const MAX_DEPTH: i32 = 8;
const MAX_DISTANCE: i32 = 80;

/// Mineshaft variant type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MineshaftType {
    /// Standard mineshaft with oak wood.
    Normal,
    /// Badlands mineshaft with dark oak wood, positioned higher.
    Mesa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    North,
    South,
    West,
    East,
}

impl Dir {
    /// Maps to the vanilla `Direction` set on `setOrientation`.
    const fn to_vanilla(self) -> Direction {
        match self {
            Dir::North => Direction::North,
            Dir::South => Direction::South,
            Dir::West => Direction::West,
            Dir::East => Direction::East,
        }
    }
}

/// Mineshaft piece kind — produced by the DFS and mapped back to vanilla's
/// `StructurePieceType` registry IDs for save-format parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PieceType {
    /// Start room (one per mineshaft).
    Room,
    /// Horizontal corridor segment.
    Corridor,
    /// Corridor crossing (possibly two-floored).
    Crossing,
    /// Stair segment.
    Stairs,
}

impl PieceType {
    /// Vanilla save-format identifier (lowercased `MSRoom` → `msroom`, etc.).
    #[must_use]
    pub const fn piece_id(self) -> &'static str {
        match self {
            Self::Room => "msroom",
            Self::Corridor => "mscorridor",
            Self::Crossing => "mscrossing",
            Self::Stairs => "msstairs",
        }
    }
}

struct PieceInfo {
    bb: BoundingBox,
    kind: MineshaftPieceKind,
    /// Distance from the start room in the DFS tree. Vanilla's `genDepth`.
    gen_depth: i32,
    /// Direction the piece was generated from. `None` for the start room.
    /// Vanilla calls `setOrientation(direction)` only on `MineShaftCorridor` and
    /// `MineShaftStairs`; rooms and crossings keep `orientation = null`.
    dir: Option<Dir>,
}

struct Pieces {
    bbs: Vec<BoundingBox>,
    infos: Vec<PieceInfo>,
    start_bb: BoundingBox,
    room_child_entrance_boxes: Vec<BoundingBox>,
}

impl Pieces {
    fn has_collision(&self, bb: &BoundingBox) -> bool {
        self.bbs.iter().any(|b| b.intersects(bb))
    }
}

/// One mineshaft piece in the generated tree.
pub struct MineshaftPieceData {
    /// Family-specific placement state.
    pub payload: MineshaftPiecePayload,
    /// World-space bounding box, already offset to the final Y position.
    pub bounding_box: BoundingBox,
    /// Distance from the start room in the DFS tree (vanilla's `genDepth`).
    pub gen_depth: i32,
    /// Vanilla `setOrientation` value: `Some` for corridors and stairs, `None`
    /// for the start room and crossings.
    pub orientation: Option<Direction>,
}

/// Placement payload shared by mineshaft start generation, persistence, and
/// feature-stage procedural placement.
#[derive(Debug, Clone, PartialEq)]
pub struct MineshaftPiecePayload {
    /// Normal oak or mesa dark-oak mineshaft.
    pub mineshaft_type: MineshaftType,
    /// Piece-specific vanilla placement state.
    pub kind: MineshaftPieceKind,
}

impl MineshaftPiecePayload {
    /// Structure piece registry id for the payload kind.
    #[must_use]
    pub const fn piece_id(&self) -> &'static str {
        self.kind.piece_type().piece_id()
    }
}

/// Piece-specific mineshaft placement state.
#[derive(Debug, Clone, PartialEq)]
pub enum MineshaftPieceKind {
    /// Start room with the child entrance openings stored by vanilla.
    Room {
        /// Child entrance boxes, offset with the room during Y adjustment.
        child_entrance_boxes: Vec<BoundingBox>,
    },
    /// Horizontal corridor segment.
    Corridor {
        /// Whether rails can generate through this corridor.
        has_rails: bool,
        /// Whether this is a cobweb-heavy cave-spider corridor.
        spider_corridor: bool,
        /// Vanilla mutable flag preventing duplicate cave-spider spawners.
        has_placed_spider: bool,
        /// Number of five-block corridor sections.
        num_sections: i32,
    },
    /// Corridor crossing.
    Crossing {
        /// Direction stored in vanilla's `"D"` field.
        direction: Direction,
        /// Whether the crossing has the upper floor.
        is_two_floored: bool,
    },
    /// Stair segment.
    Stairs,
}

impl MineshaftPieceKind {
    /// Vanilla structure piece type represented by this payload.
    #[must_use]
    pub const fn piece_type(&self) -> PieceType {
        match self {
            Self::Room { .. } => PieceType::Room,
            Self::Corridor { .. } => PieceType::Corridor,
            Self::Crossing { .. } => PieceType::Crossing,
            Self::Stairs => PieceType::Stairs,
        }
    }
}

/// Result of mineshaft generation.
pub struct MineshaftResult {
    /// Biome check position `(block_x, block_y, block_z)`.
    pub biome_check_pos: (i32, i32, i32),
    /// Pieces in DFS order, offset to final Y position.
    pub pieces: Vec<MineshaftPieceData>,
}

/// Generates mineshaft pieces and returns the biome check position + piece data.
pub fn find_generation_point(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    mtype: MineshaftType,
    sea_level: i32,
    min_y: i32,
    get_surface_height: &mut dyn FnMut(i32, i32) -> i32,
) -> MineshaftResult {
    rng.next_f64(); // vanilla: consumed but unused

    let middle_x = chunk_x * 16 + 8;
    let min_z = chunk_z * 16;
    let room_bb = create_room_bb(rng, chunk_x * 16 + 2, chunk_z * 16 + 2);

    let mut pieces = Pieces {
        bbs: vec![room_bb],
        infos: vec![PieceInfo {
            bb: room_bb,
            kind: MineshaftPieceKind::Room {
                child_entrance_boxes: Vec::new(),
            },
            gen_depth: 0,
            dir: None,
        }],
        start_bb: room_bb,
        room_child_entrance_boxes: Vec::new(),
    };
    room_add_children(&mut pieces, rng, room_bb);

    let mut overall = pieces.bbs[0];
    for bb in &pieces.bbs[1..] {
        overall = union_bb(overall, *bb);
    }

    let y_offset = if mtype == MineshaftType::Mesa {
        // Vanilla's BoundingBox.getCenter(): min + (max - min + 1) / 2.
        let center_x = overall.min_x + (overall.max_x - overall.min_x + 1) / 2;
        let center_z = overall.min_z + (overall.max_z - overall.min_z + 1) / 2;
        let surface_height = get_surface_height(center_x, center_z);
        let target = if surface_height <= sea_level {
            sea_level
        } else {
            rng.next_i32_between(sea_level, surface_height)
        };
        let center_y = overall.min_y + (overall.max_y - overall.min_y + 1) / 2;
        target - center_y
    } else {
        // moveBelowSeaLevel(seaLevel, minY, offset=10).
        let max_y = sea_level - 10;
        let mut y1_pos = (overall.max_y - overall.min_y + 1) + min_y + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        y1_pos - overall.max_y
    };

    MineshaftResult {
        biome_check_pos: (middle_x, 50 + y_offset, min_z),
        pieces: pieces
            .infos
            .iter()
            .map(|info| MineshaftPieceData {
                payload: MineshaftPiecePayload {
                    mineshaft_type: mtype,
                    kind: offset_piece_kind(
                        &info.kind,
                        &pieces.room_child_entrance_boxes,
                        y_offset,
                    ),
                },
                bounding_box: BoundingBox::new(
                    info.bb.min_x,
                    info.bb.min_y + y_offset,
                    info.bb.min_z,
                    info.bb.max_x,
                    info.bb.max_y + y_offset,
                    info.bb.max_z,
                ),
                gen_depth: info.gen_depth,
                orientation: info.dir.map(Dir::to_vanilla),
            })
            .collect(),
    }
}

fn offset_piece_kind(
    kind: &MineshaftPieceKind,
    room_child_entrance_boxes: &[BoundingBox],
    y_offset: i32,
) -> MineshaftPieceKind {
    match kind {
        MineshaftPieceKind::Room { .. } => MineshaftPieceKind::Room {
            child_entrance_boxes: room_child_entrance_boxes
                .iter()
                .map(|bb| move_bb(*bb, 0, y_offset, 0))
                .collect(),
        },
        other => other.clone(),
    }
}

fn create_room_bb(rng: &mut LegacyRandom, west: i32, north: i32) -> BoundingBox {
    BoundingBox::new(
        west,
        50,
        north,
        west + 7 + rng.next_i32_bounded(6),
        54 + rng.next_i32_bounded(6),
        north + 7 + rng.next_i32_bounded(6),
    )
}

/// Matches vanilla `MineShaftRoom.addChildren` exactly.
fn room_add_children(pieces: &mut Pieces, rng: &mut LegacyRandom, bb: BoundingBox) {
    let x_span = bb.max_x - bb.min_x + 1;
    let z_span = bb.max_z - bb.min_z + 1;
    let height_space = ((bb.max_y - bb.min_y + 1) - 3 - 1).max(1);

    // Each of the 4 walls: iterate over the relevant span, drop openings.
    for (dir, span) in [
        (Dir::North, x_span),
        (Dir::South, x_span),
        (Dir::West, z_span),
        (Dir::East, z_span),
    ] {
        let mut pos = 0;
        while pos < span {
            pos += rng.next_i32_bounded(span);
            if pos + 3 > span {
                break;
            }
            let fy = bb.min_y + rng.next_i32_bounded(height_space) + 1;
            let (fx, fz) = match dir {
                Dir::North => (bb.min_x + pos, bb.min_z - 1),
                Dir::South => (bb.min_x + pos, bb.max_z + 1),
                Dir::West => (bb.min_x - 1, bb.min_z + pos),
                Dir::East => (bb.max_x + 1, bb.min_z + pos),
            };
            if let Some(child_bb) = generate_and_add(pieces, rng, fx, fy, fz, dir, 0) {
                let entrance = match dir {
                    Dir::North => BoundingBox::new(
                        child_bb.min_x,
                        child_bb.min_y,
                        bb.min_z,
                        child_bb.max_x,
                        child_bb.max_y,
                        bb.min_z + 1,
                    ),
                    Dir::South => BoundingBox::new(
                        child_bb.min_x,
                        child_bb.min_y,
                        bb.max_z - 1,
                        child_bb.max_x,
                        child_bb.max_y,
                        bb.max_z,
                    ),
                    Dir::West => BoundingBox::new(
                        bb.min_x,
                        child_bb.min_y,
                        child_bb.min_z,
                        bb.min_x + 1,
                        child_bb.max_y,
                        child_bb.max_z,
                    ),
                    Dir::East => BoundingBox::new(
                        bb.max_x - 1,
                        child_bb.min_y,
                        child_bb.min_z,
                        bb.max_x,
                        child_bb.max_y,
                        child_bb.max_z,
                    ),
                };
                pieces.room_child_entrance_boxes.push(entrance);
            }
            pos += 4;
        }
    }
}

/// Vanilla's `generateAndAddPiece` — DFS: create a piece, add it, recurse.
/// `createRandomShaftPiece` is if/else if/else (no fallthrough).
fn generate_and_add(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    depth: i32,
) -> Option<BoundingBox> {
    if depth > MAX_DEPTH
        || (foot_x - pieces.start_bb.min_x).abs() > MAX_DISTANCE
        || (foot_z - pieces.start_bb.min_z).abs() > MAX_DISTANCE
    {
        return None;
    }
    let roll = rng.next_i32_bounded(100);
    if roll >= 80 {
        try_add_crossing(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1)
    } else if roll >= 70 {
        try_add_stairs(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1)
    } else {
        try_add_corridor(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1)
    }
}

fn push_piece(
    pieces: &mut Pieces,
    bb: BoundingBox,
    kind: MineshaftPieceKind,
    gen_depth: i32,
    dir: Dir,
) {
    pieces.bbs.push(bb);
    // Vanilla only calls `setOrientation` on corridors and stairs; rooms have
    // no direction at all and crossings store it internally without setting
    // orientation, so both serialize as `O = -1`.
    let saved_dir = match &kind {
        MineshaftPieceKind::Corridor { .. } | MineshaftPieceKind::Stairs => Some(dir),
        MineshaftPieceKind::Room { .. } | MineshaftPieceKind::Crossing { .. } => None,
    };
    pieces.infos.push(PieceInfo {
        bb,
        kind,
        gen_depth,
        dir: saved_dir,
    });
}

const fn corridor_num_sections(bb: BoundingBox, dir: Dir) -> i32 {
    match dir {
        Dir::North | Dir::South => (bb.max_z - bb.min_z + 1) / 5,
        Dir::West | Dir::East => (bb.max_x - bb.min_x + 1) / 5,
    }
}

fn corridor_payload(bb: BoundingBox, dir: Dir, rng: &mut LegacyRandom) -> MineshaftPieceKind {
    let has_rails = rng.next_i32_bounded(3) == 0;
    let spider_corridor = !has_rails && rng.next_i32_bounded(23) == 0;
    MineshaftPieceKind::Corridor {
        has_rails,
        spider_corridor,
        has_placed_spider: false,
        num_sections: corridor_num_sections(bb, dir),
    }
}

const fn crossing_payload(dir: Dir, is_two_floored: bool) -> MineshaftPieceKind {
    MineshaftPieceKind::Crossing {
        direction: dir.to_vanilla(),
        is_two_floored,
    }
}

const fn stairs_payload() -> MineshaftPieceKind {
    MineshaftPieceKind::Stairs
}

fn try_add_corridor(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> Option<BoundingBox> {
    let mut corridor_length = rng.next_i32_bounded(3) + 2;
    while corridor_length > 0 {
        let block_length = corridor_length * 5;
        let bb = move_bb(
            match dir {
                Dir::North => BoundingBox::new(0, 0, -(block_length - 1), 2, 2, 0),
                Dir::South => BoundingBox::new(0, 0, 0, 2, 2, block_length - 1),
                Dir::West => BoundingBox::new(-(block_length - 1), 0, 0, 0, 2, 2),
                Dir::East => BoundingBox::new(0, 0, 0, block_length - 1, 2, 2),
            },
            foot_x,
            foot_y,
            foot_z,
        );
        if !pieces.has_collision(&bb) {
            let kind = corridor_payload(bb, dir, rng);
            push_piece(pieces, bb, kind, gen_depth, dir);
            corridor_add_children(pieces, rng, bb, dir, gen_depth);
            return Some(bb);
        }
        corridor_length -= 1;
    }
    None
}

fn try_add_crossing(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> Option<BoundingBox> {
    let is_two_floored = rng.next_i32_bounded(4) == 0;
    let y1 = if is_two_floored { 6 } else { 2 };
    let bb = move_bb(
        match dir {
            Dir::North => BoundingBox::new(-1, 0, -4, 3, y1, 0),
            Dir::South => BoundingBox::new(-1, 0, 0, 3, y1, 4),
            Dir::West => BoundingBox::new(-4, 0, -1, 0, y1, 3),
            Dir::East => BoundingBox::new(0, 0, -1, 4, y1, 3),
        },
        foot_x,
        foot_y,
        foot_z,
    );
    if pieces.has_collision(&bb) {
        return None;
    }
    push_piece(
        pieces,
        bb,
        crossing_payload(dir, is_two_floored),
        gen_depth,
        dir,
    );
    crossing_add_children(pieces, rng, bb, dir, gen_depth, is_two_floored);
    Some(bb)
}

fn try_add_stairs(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> Option<BoundingBox> {
    let bb = move_bb(
        match dir {
            Dir::North => BoundingBox::new(0, -5, -8, 2, 2, 0),
            Dir::South => BoundingBox::new(0, -5, 0, 2, 2, 8),
            Dir::West => BoundingBox::new(-8, -5, 0, 0, 2, 2),
            Dir::East => BoundingBox::new(0, -5, 0, 8, 2, 2),
        },
        foot_x,
        foot_y,
        foot_z,
    );
    if pieces.has_collision(&bb) {
        return None;
    }
    push_piece(pieces, bb, stairs_payload(), gen_depth, dir);
    stairs_add_children(pieces, rng, bb, dir, gen_depth);
    Some(bb)
}

/// Matches vanilla `MineShaftCorridor.addChildren` exactly.
fn corridor_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
) {
    // End piece.
    let end_selection = rng.next_i32_bounded(4);
    let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
    #[expect(
        clippy::match_same_arms,
        reason = "arms kept per-direction to mirror vanilla's switch dispatch"
    )]
    let (fx, fz, d) = match (dir, end_selection) {
        (Dir::North, 0 | 1) => (bb.min_x, bb.min_z - 1, Dir::North),
        (Dir::North, 2) => (bb.min_x - 1, bb.min_z, Dir::West),
        (Dir::North, _) => (bb.max_x + 1, bb.min_z, Dir::East),
        (Dir::South, 0 | 1) => (bb.min_x, bb.max_z + 1, Dir::South),
        (Dir::South, 2) => (bb.min_x - 1, bb.max_z - 3, Dir::West),
        (Dir::South, _) => (bb.max_x + 1, bb.max_z - 3, Dir::East),
        (Dir::West, 0 | 1) => (bb.min_x - 1, bb.min_z, Dir::West),
        (Dir::West, 2) => (bb.min_x, bb.min_z - 1, Dir::North),
        (Dir::West, _) => (bb.min_x, bb.max_z + 1, Dir::South),
        (Dir::East, 0 | 1) => (bb.max_x + 1, bb.min_z, Dir::East),
        (Dir::East, 2) => (bb.max_x - 3, bb.min_z - 1, Dir::North),
        (Dir::East, _) => (bb.max_x - 3, bb.max_z + 1, Dir::South),
    };
    let _ = generate_and_add(pieces, rng, fx, fy, fz, d, depth);

    // Perpendicular branches along the corridor.
    if depth >= MAX_DEPTH {
        return;
    }
    match dir {
        Dir::North | Dir::South => {
            let mut z = bb.min_z + 3;
            while z + 3 <= bb.max_z {
                match rng.next_i32_bounded(5) {
                    0 => {
                        let _ = generate_and_add(
                            pieces,
                            rng,
                            bb.min_x - 1,
                            bb.min_y,
                            z,
                            Dir::West,
                            depth + 1,
                        );
                    }
                    1 => {
                        let _ = generate_and_add(
                            pieces,
                            rng,
                            bb.max_x + 1,
                            bb.min_y,
                            z,
                            Dir::East,
                            depth + 1,
                        );
                    }
                    _ => {}
                }
                z += 5;
            }
        }
        Dir::West | Dir::East => {
            let mut x = bb.min_x + 3;
            while x + 3 <= bb.max_x {
                match rng.next_i32_bounded(5) {
                    0 => {
                        let _ = generate_and_add(
                            pieces,
                            rng,
                            x,
                            bb.min_y,
                            bb.min_z - 1,
                            Dir::North,
                            depth + 1,
                        );
                    }
                    1 => {
                        let _ = generate_and_add(
                            pieces,
                            rng,
                            x,
                            bb.min_y,
                            bb.max_z + 1,
                            Dir::South,
                            depth + 1,
                        );
                    }
                    _ => {}
                }
                x += 5;
            }
        }
    }
}

/// Matches vanilla `MineShaftCrossing.addChildren` exactly.
fn crossing_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
    is_two_floored: bool,
) {
    // Three outgoing branches at y = bb.min_y per direction.
    let outs: [(i32, i32, Dir); 3] = match dir {
        Dir::North => [
            (bb.min_x + 1, bb.min_z - 1, Dir::North),
            (bb.min_x - 1, bb.min_z + 1, Dir::West),
            (bb.max_x + 1, bb.min_z + 1, Dir::East),
        ],
        Dir::South => [
            (bb.min_x + 1, bb.max_z + 1, Dir::South),
            (bb.min_x - 1, bb.min_z + 1, Dir::West),
            (bb.max_x + 1, bb.min_z + 1, Dir::East),
        ],
        Dir::West => [
            (bb.min_x + 1, bb.min_z - 1, Dir::North),
            (bb.min_x + 1, bb.max_z + 1, Dir::South),
            (bb.min_x - 1, bb.min_z + 1, Dir::West),
        ],
        Dir::East => [
            (bb.min_x + 1, bb.min_z - 1, Dir::North),
            (bb.min_x + 1, bb.max_z + 1, Dir::South),
            (bb.max_x + 1, bb.min_z + 1, Dir::East),
        ],
    };
    for (x, z, d) in outs {
        let _ = generate_and_add(pieces, rng, x, bb.min_y, z, d, depth);
    }

    if is_two_floored {
        // Each branch consumes a next_bool in order (N, W, E, S).
        for (x, z, d) in [
            (bb.min_x + 1, bb.min_z - 1, Dir::North),
            (bb.min_x - 1, bb.min_z + 1, Dir::West),
            (bb.max_x + 1, bb.min_z + 1, Dir::East),
            (bb.min_x + 1, bb.max_z + 1, Dir::South),
        ] {
            if rng.next_bool() {
                let _ = generate_and_add(pieces, rng, x, bb.min_y + 4, z, d, depth);
            }
        }
    }
}

/// Matches vanilla `MineShaftStairs.addChildren` exactly.
fn stairs_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
) {
    let (x, z) = match dir {
        Dir::North => (bb.min_x, bb.min_z - 1),
        Dir::South => (bb.min_x, bb.max_z + 1),
        Dir::West => (bb.min_x - 1, bb.min_z),
        Dir::East => (bb.max_x + 1, bb.min_z),
    };
    let _ = generate_and_add(pieces, rng, x, bb.min_y, z, dir, depth);
}

// --- Helpers ---

const fn move_bb(bb: BoundingBox, dx: i32, dy: i32, dz: i32) -> BoundingBox {
    BoundingBox::new(
        bb.min_x + dx,
        bb.min_y + dy,
        bb.min_z + dz,
        bb.max_x + dx,
        bb.max_y + dy,
        bb.max_z + dz,
    )
}

fn union_bb(a: BoundingBox, b: BoundingBox) -> BoundingBox {
    BoundingBox::new(
        a.min_x.min(b.min_x),
        a.min_y.min(b.min_y),
        a.min_z.min(b.min_z),
        a.max_x.max(b.max_x),
        a.max_y.max(b.max_y),
        a.max_z.max(b.max_z),
    )
}

/// Registered under `"minecraft:mineshaft"`.
pub struct MineshaftStructure;

impl Structure for MineshaftStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let StructureConfigData::Mineshaft { mineshaft_type } = &structure.config else {
            return None;
        };
        let mtype = match mineshaft_type {
            MineshaftTypeData::Normal => MineshaftType::Normal,
            MineshaftTypeData::Mesa => MineshaftType::Mesa,
        };

        let mut get_height = |x: i32, z: i32| ctx.terrain_surface_height(x, z, false);

        let result = find_generation_point(
            rng,
            ctx.chunk_x(),
            ctx.chunk_z(),
            mtype,
            ctx.sea_level(),
            ctx.min_y(),
            &mut get_height,
        );

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        Some(GenerationStub {
            position: result.biome_check_pos,
            pieces: result
                .pieces
                .into_iter()
                .map(|p| {
                    let payload = p.payload;
                    StructurePiece {
                        piece_type: Identifier::new_static("minecraft", payload.piece_id()),
                        bounding_box: p.bounding_box,
                        gen_depth: p.gen_depth,
                        orientation: p.orientation,
                        payload: StructurePiecePayload::Procedural(ProceduralPieceData::Mineshaft(
                            payload,
                        )),
                        ground_level_delta: 0,
                        junctions: Vec::new(),
                        projection: None,
                    }
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seed 13579 at chunk (0, 0): 92 pieces, overall BB, Y-offset = -70.
    /// Vanilla values from the extractor mixin trace.
    #[test]
    fn mineshaft_matches_vanilla_seed_13579_chunk_0_0() {
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(13579, 0, 0);
        rng.next_f64();

        let room_bb = create_room_bb(&mut rng, 2, 2);
        assert_eq!(room_bb, BoundingBox::new(2, 50, 2, 13, 56, 9));

        let mut pieces = Pieces {
            bbs: vec![room_bb],
            infos: vec![PieceInfo {
                bb: room_bb,
                kind: MineshaftPieceKind::Room {
                    child_entrance_boxes: Vec::new(),
                },
                gen_depth: 0,
                dir: None,
            }],
            start_bb: room_bb,
            room_child_entrance_boxes: Vec::new(),
        };
        room_add_children(&mut pieces, &mut rng, room_bb);
        assert_eq!(pieces.bbs.len(), 92);

        let mut overall = pieces.bbs[0];
        for bb in &pieces.bbs[1..] {
            overall = union_bb(overall, *bb);
        }
        assert_eq!(overall, BoundingBox::new(-45, 42, -74, 60, 59, 41));

        let max_y = 63 - 10;
        let mut y1_pos = (overall.max_y - overall.min_y + 1) + (-64) + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        let y_offset = y1_pos - overall.max_y;
        assert_eq!(y_offset, -70);
        assert_eq!(50 + y_offset, -20);
    }

    #[test]
    fn mineshaft_generation_captures_piece_payload_state() {
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(13579, 0, 0);
        let mut surface_height = |_, _| 63;

        let result = find_generation_point(
            &mut rng,
            0,
            0,
            MineshaftType::Normal,
            63,
            -64,
            &mut surface_height,
        );

        let MineshaftPieceKind::Room {
            child_entrance_boxes,
        } = &result.pieces[0].payload.kind
        else {
            panic!("first mineshaft piece should be the start room");
        };
        assert!(!child_entrance_boxes.is_empty());

        let corridor = result
            .pieces
            .iter()
            .find_map(|piece| match &piece.payload.kind {
                MineshaftPieceKind::Corridor {
                    has_rails,
                    spider_corridor,
                    has_placed_spider,
                    num_sections,
                } => Some((
                    *has_rails,
                    *spider_corridor,
                    *has_placed_spider,
                    *num_sections,
                )),
                _ => None,
            })
            .expect("seed should generate at least one corridor");
        assert!(!corridor.2);
        assert!(corridor.3 > 0);
        assert!(
            result
                .pieces
                .iter()
                .all(|piece| piece.payload.mineshaft_type == MineshaftType::Normal)
        );
    }
}
