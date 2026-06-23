//! Stronghold piece generation. Vanilla's `StrongholdPieces` recursive BFS;
//! produces bounding boxes only (no blocks).

use glam::IVec3;
use steel_registry::structure::StructureData;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier};

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload,
};

const MAX_DEPTH: i32 = 50;
const MAX_DISTANCE: i32 = 112;
const LOWEST_Y: i32 = 10;

const HORIZONTAL_DIRS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

fn random_horizontal(rng: &mut LegacyRandom) -> Direction {
    HORIZONTAL_DIRS[rng.next_i32_bounded(4) as usize]
}

/// Vanilla's `BoundingBox.orientBox`.
const fn orient_box(foot: IVec3, off: IVec3, size: IVec3, dir: Direction) -> BoundingBox {
    let fx = foot.x;
    let fy = foot.y + off.y;
    let fz = foot.z;
    let w = size.x;
    let h = size.y;
    let d = size.z;
    match dir {
        Direction::North => BoundingBox::new(
            IVec3::new(fx + off.x, fy, fz - d + 1 + off.z),
            IVec3::new(fx + w - 1 + off.x, fy + h - 1, fz + off.z),
        ),
        Direction::West => BoundingBox::new(
            IVec3::new(fx - d + 1 + off.z, fy, fz + off.x),
            IVec3::new(fx + off.z, fy + h - 1, fz + w - 1 + off.x),
        ),
        Direction::East => BoundingBox::new(
            IVec3::new(fx + off.z, fy, fz + off.x),
            IVec3::new(fx + d - 1 + off.z, fy + h - 1, fz + w - 1 + off.x),
        ),
        // South + default
        _ => BoundingBox::new(
            IVec3::new(fx + off.x, fy, fz + off.z),
            IVec3::new(fx + w - 1 + off.x, fy + h - 1, fz + d - 1 + off.z),
        ),
    }
}

const fn is_ok(bb: &BoundingBox) -> bool {
    bb.min_y() > LOWEST_Y
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PT {
    Straight,
    Prison,
    LeftTurn,
    RightTurn,
    RoomCrossing,
    StraightStairs,
    StairsDown,
    FiveCrossing,
    ChestCorridor,
    Library,
    Portal,
    Filler,
}

/// Vanilla `StrongholdPiece.SmallDoorType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrongholdSmallDoorType {
    /// Three-block cave-air opening.
    Opening,
    /// Oak door framed by stone bricks.
    WoodDoor,
    /// Iron-bar grate opening.
    Grates,
    /// Iron door with stone buttons.
    IronDoor,
}

impl StrongholdSmallDoorType {
    fn random(rng: &mut LegacyRandom) -> Self {
        match rng.next_i32_bounded(5) {
            2 => Self::WoodDoor,
            3 => Self::Grates,
            4 => Self::IronDoor,
            _ => Self::Opening,
        }
    }
}

/// Vanilla stronghold piece runtime state needed by `postProcess`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrongholdPieceData {
    /// Straight corridor with optional side exits.
    Straight {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `leftChild`.
        left_child: bool,
        /// Vanilla `rightChild`.
        right_child: bool,
    },
    /// Prison hall.
    PrisonHall {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
    },
    /// Left turn.
    LeftTurn {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
    },
    /// Right turn.
    RightTurn {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
    },
    /// Room crossing with one of five vanilla decorations.
    RoomCrossing {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `type`.
        crossing_type: i32,
    },
    /// Straight stair corridor.
    StraightStairsDown {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
    },
    /// Descending stairs, including the source/start piece.
    StairsDown {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `isSource`.
        is_source: bool,
    },
    /// Five-way crossing with low/high side exits.
    FiveCrossing {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `leftLow`.
        left_low: bool,
        /// Vanilla `leftHigh`.
        left_high: bool,
        /// Vanilla `rightLow`.
        right_low: bool,
        /// Vanilla `rightHigh`.
        right_high: bool,
    },
    /// Corridor containing a loot chest.
    ChestCorridor {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `hasPlacedChest`.
        has_placed_chest: bool,
    },
    /// Library room.
    Library {
        /// Vanilla `entryDoor`.
        entry_door: StrongholdSmallDoorType,
        /// Vanilla `isTall`.
        is_tall: bool,
    },
    /// End portal room.
    PortalRoom {
        /// Vanilla `hasPlacedSpawner`.
        has_placed_spawner: bool,
    },
    /// Collision filler corridor.
    FillerCorridor {
        /// Vanilla `steps`.
        steps: i32,
    },
}

impl StrongholdPieceData {
    const fn piece_type(self) -> PT {
        match self {
            Self::Straight { .. } => PT::Straight,
            Self::PrisonHall { .. } => PT::Prison,
            Self::LeftTurn { .. } => PT::LeftTurn,
            Self::RightTurn { .. } => PT::RightTurn,
            Self::RoomCrossing { .. } => PT::RoomCrossing,
            Self::StraightStairsDown { .. } => PT::StraightStairs,
            Self::StairsDown { .. } => PT::StairsDown,
            Self::FiveCrossing { .. } => PT::FiveCrossing,
            Self::ChestCorridor { .. } => PT::ChestCorridor,
            Self::Library { .. } => PT::Library,
            Self::PortalRoom { .. } => PT::Portal,
            Self::FillerCorridor { .. } => PT::Filler,
        }
    }

    /// Returns the vanilla structure piece id for this payload.
    #[must_use]
    pub const fn piece_id(self) -> &'static str {
        match self {
            Self::StairsDown {
                is_source: true, ..
            } => "shstart",
            Self::StairsDown { .. } => "shsd",
            Self::Straight { .. } => "shs",
            Self::PrisonHall { .. } => "shph",
            Self::LeftTurn { .. } => "shlt",
            Self::RightTurn { .. } => "shrt",
            Self::RoomCrossing { .. } => "shrc",
            Self::StraightStairsDown { .. } => "shssd",
            Self::FiveCrossing { .. } => "sh5c",
            Self::ChestCorridor { .. } => "shcc",
            Self::Library { .. } => "shli",
            Self::PortalRoom { .. } => "shpr",
            Self::FillerCorridor { .. } => "shfc",
        }
    }

    const fn start() -> Self {
        Self::StairsDown {
            entry_door: StrongholdSmallDoorType::Opening,
            is_source: true,
        }
    }
}

struct PieceWeight {
    pt: PT,
    weight: i32,
    max: i32,
    count: i32,
    min_depth: i32,
}
impl PieceWeight {
    const fn can(&self, depth: i32) -> bool {
        (self.max == 0 || self.count < self.max) && depth >= self.min_depth
    }
}

fn weights() -> Vec<PieceWeight> {
    #[rustfmt::skip]
    const W: &[(PT, i32, i32, i32)] = &[
        (PT::Straight,       40, 0, 0),
        (PT::Prison,          5, 5, 0),
        (PT::LeftTurn,       20, 0, 0),
        (PT::RightTurn,      20, 0, 0),
        (PT::RoomCrossing,   10, 6, 0),
        (PT::StraightStairs,  5, 5, 0),
        (PT::StairsDown,      5, 5, 0),
        (PT::FiveCrossing,    5, 4, 0),
        (PT::ChestCorridor,   5, 4, 0),
        (PT::Library,        10, 2, 5),
        (PT::Portal,         20, 1, 6),
    ];
    W.iter()
        .map(|&(pt, weight, max, min_depth)| PieceWeight {
            pt,
            weight,
            max,
            count: 0,
            min_depth,
        })
        .collect()
}
struct Piece {
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    data: StrongholdPieceData,
}

impl Piece {
    const fn new(bb: BoundingBox, dir: Direction, depth: i32, data: StrongholdPieceData) -> Self {
        Self {
            bb,
            dir,
            depth,
            data,
        }
    }

    const fn pt(&self) -> PT {
        self.data.piece_type()
    }
}

struct State {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    wts: Vec<PieceWeight>,
    start_bb: BoundingBox,
    prev_pt: Option<PT>,
    has_portal: bool,
    imposed: Option<PT>,
    total_weight: i32,
}

impl State {
    fn collides(&self, bb: BoundingBox) -> bool {
        self.pieces.iter().any(|p| p.bb.intersects(bb))
    }

    /// Vanilla's `updatePieceWeight`. STOPS generation when no limited pieces
    /// have room, even if unlimited pieces remain.
    fn update_weights(&mut self) -> bool {
        let mut has_any = false;
        self.total_weight = 0;
        for w in &self.wts {
            if w.max > 0 && w.count < w.max {
                has_any = true;
            }
            self.total_weight += w.weight;
        }
        has_any
    }
}

fn find_box(pt: PT, s: &State, foot: IVec3, dir: Direction) -> Option<BoundingBox> {
    let bb = match pt {
        PT::Straight | PT::ChestCorridor => {
            orient_box(foot, IVec3::new(-1, -1, 0), IVec3::new(5, 5, 7), dir)
        }
        PT::StairsDown => orient_box(foot, IVec3::new(-1, -7, 0), IVec3::new(5, 11, 5), dir),
        PT::StraightStairs => orient_box(foot, IVec3::new(-1, -7, 0), IVec3::new(5, 11, 8), dir),
        PT::LeftTurn | PT::RightTurn => {
            orient_box(foot, IVec3::new(-1, -1, 0), IVec3::new(5, 5, 5), dir)
        }
        PT::RoomCrossing => orient_box(foot, IVec3::new(-4, -1, 0), IVec3::new(11, 7, 11), dir),
        PT::Prison => orient_box(foot, IVec3::new(-1, -1, 0), IVec3::new(9, 5, 11), dir),
        PT::FiveCrossing => orient_box(foot, IVec3::new(-4, -3, 0), IVec3::new(10, 9, 11), dir),
        PT::Portal => orient_box(foot, IVec3::new(-4, -1, 0), IVec3::new(11, 8, 16), dir),
        PT::Library => {
            let tall = orient_box(foot, IVec3::new(-4, -1, 0), IVec3::new(14, 11, 15), dir);
            if is_ok(&tall) && !s.collides(tall) {
                return Some(tall);
            }
            orient_box(foot, IVec3::new(-4, -1, 0), IVec3::new(14, 6, 15), dir)
        }
        PT::Filler => {
            let full_box = orient_box(foot, IVec3::new(-1, -1, 0), IVec3::new(5, 5, 4), dir);
            let collision = s.pieces.iter().find(|p| p.bb.intersects(full_box))?;
            if collision.bb.min_y() != full_box.min_y() {
                return None;
            }
            for d in (1..=2).rev() {
                let b = orient_box(foot, IVec3::new(-1, -1, 0), IVec3::new(5, 5, d), dir);
                if !collision.bb.intersects(b) {
                    return Some(orient_box(
                        foot,
                        IVec3::new(-1, -1, 0),
                        IVec3::new(5, 5, d + 1),
                        dir,
                    ));
                }
            }
            return None;
        }
    };
    if is_ok(&bb) && !s.collides(bb) {
        Some(bb)
    } else {
        None
    }
}

fn create_piece(
    pt: PT,
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    rng: &mut LegacyRandom,
) -> Piece {
    let data = match pt {
        PT::Straight => {
            let entry_door = StrongholdSmallDoorType::random(rng);
            StrongholdPieceData::Straight {
                entry_door,
                left_child: rng.next_i32_bounded(2) == 0,
                right_child: rng.next_i32_bounded(2) == 0,
            }
        }
        PT::FiveCrossing => {
            let entry_door = StrongholdSmallDoorType::random(rng);
            StrongholdPieceData::FiveCrossing {
                entry_door,
                left_low: rng.next_bool(),
                left_high: rng.next_bool(),
                right_low: rng.next_bool(),
                right_high: rng.next_i32_bounded(3) > 0,
            }
        }
        PT::RoomCrossing => {
            let entry_door = StrongholdSmallDoorType::random(rng);
            StrongholdPieceData::RoomCrossing {
                entry_door,
                crossing_type: rng.next_i32_bounded(5),
            }
        }
        PT::Library => {
            let entry_door = StrongholdSmallDoorType::random(rng);
            StrongholdPieceData::Library {
                entry_door,
                is_tall: bb.height() > 6,
            }
        }
        PT::Portal => StrongholdPieceData::PortalRoom {
            has_placed_spawner: false,
        },
        PT::Filler => StrongholdPieceData::FillerCorridor {
            steps: if matches!(dir, Direction::North | Direction::South) {
                bb.depth()
            } else {
                bb.width()
            },
        },
        PT::StairsDown => StrongholdPieceData::StairsDown {
            entry_door: StrongholdSmallDoorType::random(rng),
            is_source: false,
        },
        PT::ChestCorridor => StrongholdPieceData::ChestCorridor {
            entry_door: StrongholdSmallDoorType::random(rng),
            has_placed_chest: false,
        },
        PT::StraightStairs => StrongholdPieceData::StraightStairsDown {
            entry_door: StrongholdSmallDoorType::random(rng),
        },
        PT::LeftTurn => StrongholdPieceData::LeftTurn {
            entry_door: StrongholdSmallDoorType::random(rng),
        },
        PT::RightTurn => StrongholdPieceData::RightTurn {
            entry_door: StrongholdSmallDoorType::random(rng),
        },
        PT::Prison => StrongholdPieceData::PrisonHall {
            entry_door: StrongholdSmallDoorType::random(rng),
        },
    };
    Piece::new(bb, dir, depth, data)
}

fn generate_piece(
    s: &mut State,
    rng: &mut LegacyRandom,
    fx: i32,
    fy: i32,
    fz: i32,
    dir: Direction,
    depth: i32,
) -> Option<Piece> {
    if !s.update_weights() {
        return None;
    }

    let foot = IVec3::new(fx, fy, fz);

    if let Some(imp) = s.imposed.take()
        && let Some(bb) = find_box(imp, s, foot, dir)
    {
        return Some(create_piece(imp, bb, dir, depth, rng));
    }

    for _ in 0..5 {
        if s.total_weight <= 0 {
            break;
        }
        let mut choice = rng.next_i32_bounded(s.total_weight);
        for wi in 0..s.wts.len() {
            choice -= s.wts[wi].weight;
            if choice < 0 {
                if !s.wts[wi].can(depth) || Some(s.wts[wi].pt) == s.prev_pt {
                    break;
                }
                if let Some(bb) = find_box(s.wts[wi].pt, s, foot, dir) {
                    let pt = s.wts[wi].pt;
                    let piece = create_piece(pt, bb, dir, depth, rng);
                    s.wts[wi].count += 1;
                    s.prev_pt = Some(pt);
                    if s.wts[wi].max > 0 && s.wts[wi].count >= s.wts[wi].max {
                        s.wts.remove(wi);
                    }
                    return Some(piece);
                }
            }
        }
    }

    if let Some(bb) = find_box(PT::Filler, s, foot, dir)
        && bb.min_y() > 1
    {
        return Some(create_piece(PT::Filler, bb, dir, depth, rng));
    }
    None
}

fn gen_and_add(
    s: &mut State,
    rng: &mut LegacyRandom,
    fx: i32,
    fy: i32,
    fz: i32,
    dir: Direction,
    depth: i32,
) {
    if depth > MAX_DEPTH
        || (fx - s.start_bb.min_x()).abs() > MAX_DISTANCE
        || (fz - s.start_bb.min_z()).abs() > MAX_DISTANCE
    {
        return;
    }
    if let Some(piece) = generate_piece(s, rng, fx, fy, fz, dir, depth) {
        if piece.pt() == PT::Portal {
            s.has_portal = true;
        }
        let idx = s.pieces.len();
        s.pieces.push(piece);
        s.pending.push(idx);
    }
}

fn add_children(s: &mut State, rng: &mut LegacyRandom, idx: usize) {
    let Piece {
        bb,
        dir,
        depth,
        data,
        ..
    } = s.pieces[idx];
    let pt = data.piece_type();
    let nw_facing = matches!(dir, Direction::North | Direction::East);

    match pt {
        PT::StairsDown => {
            if depth == 0 {
                s.imposed = Some(PT::FiveCrossing);
            }
            fwd(s, rng, bb, dir, depth, 1, 1);
        }
        PT::StraightStairs | PT::ChestCorridor | PT::Prison => {
            fwd(s, rng, bb, dir, depth, 1, 1);
        }
        PT::Straight => {
            let StrongholdPieceData::Straight {
                left_child: lc,
                right_child: rc,
                ..
            } = data
            else {
                return;
            };
            fwd(s, rng, bb, dir, depth, 1, 1);
            if lc {
                left(s, rng, bb, dir, depth, 1, 2);
            }
            if rc {
                right(s, rng, bb, dir, depth, 1, 2);
            }
        }
        PT::LeftTurn => {
            if nw_facing {
                left(s, rng, bb, dir, depth, 1, 1);
            } else {
                right(s, rng, bb, dir, depth, 1, 1);
            }
        }
        PT::RightTurn => {
            if nw_facing {
                right(s, rng, bb, dir, depth, 1, 1);
            } else {
                left(s, rng, bb, dir, depth, 1, 1);
            }
        }
        PT::RoomCrossing => {
            fwd(s, rng, bb, dir, depth, 4, 1);
            left(s, rng, bb, dir, depth, 1, 4);
            right(s, rng, bb, dir, depth, 1, 4);
        }
        PT::FiveCrossing => {
            let StrongholdPieceData::FiveCrossing {
                left_low: ll,
                left_high: lh,
                right_low: rl,
                right_high: rh,
                ..
            } = data
            else {
                return;
            };
            let (za, zb) = if matches!(dir, Direction::West | Direction::North) {
                (5, 3)
            } else {
                (3, 5)
            };
            fwd(s, rng, bb, dir, depth, 5, 1);
            if ll {
                left(s, rng, bb, dir, depth, za, 1);
            }
            if lh {
                left(s, rng, bb, dir, depth, zb, 7);
            }
            if rl {
                right(s, rng, bb, dir, depth, za, 1);
            }
            if rh {
                right(s, rng, bb, dir, depth, zb, 7);
            }
        }
        PT::Library | PT::Filler | PT::Portal => {}
    }
}

fn fwd(
    s: &mut State,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    x_off: i32,
    y_off: i32,
) {
    let (fx, fz) = match dir {
        Direction::North => (bb.min_x() + x_off, bb.min_z() - 1),
        Direction::South => (bb.min_x() + x_off, bb.max_z() + 1),
        Direction::West => (bb.min_x() - 1, bb.min_z() + x_off),
        Direction::East => (bb.max_x() + 1, bb.min_z() + x_off),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y() + y_off, fz, dir, depth + 1);
}

fn left(
    s: &mut State,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
) {
    let (fx, fz, d) = match dir {
        Direction::North | Direction::South => {
            (bb.min_x() - 1, bb.min_z() + z_off, Direction::West)
        }
        Direction::West | Direction::East => (bb.min_x() + z_off, bb.min_z() - 1, Direction::North),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y() + y_off, fz, d, depth + 1);
}

fn right(
    s: &mut State,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    y_off: i32,
    z_off: i32,
) {
    let (fx, fz, d) = match dir {
        Direction::North | Direction::South => {
            (bb.max_x() + 1, bb.min_z() + z_off, Direction::East)
        }
        Direction::West | Direction::East => (bb.min_x() + z_off, bb.max_z() + 1, Direction::South),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y() + y_off, fz, d, depth + 1);
}

/// One generated stronghold piece.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrongholdGeneratedPiece {
    /// World-space bounding box.
    pub bounding_box: BoundingBox,
    /// Horizontal orientation.
    pub orientation: Direction,
    /// Vanilla generation depth.
    pub gen_depth: i32,
    /// Stronghold piece placement payload.
    pub data: StrongholdPieceData,
}

/// All stronghold pieces for a chunk. Vanilla calls
/// `setOrientation(direction)` on every stronghold piece, and threads
/// `genDepth` through the DFS via each subclass's `createPiece` helper.
#[must_use]
pub fn generate_pieces(seed: i64, chunk_x: i32, chunk_z: i32) -> Vec<StrongholdGeneratedPiece> {
    let west = chunk_x * 16 + 2;
    let north = chunk_z * 16 + 2;

    let mut tries = 0i64;
    loop {
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(seed.wrapping_add(tries), chunk_x, chunk_z);
        tries += 1;

        let start_dir = random_horizontal(&mut rng);
        let start_bb = BoundingBox::new(
            IVec3::new(west, 64, north),
            IVec3::new(west + 4, 74, north + 4),
        );

        let mut s = State {
            pieces: vec![Piece::new(
                start_bb,
                start_dir,
                0,
                StrongholdPieceData::start(),
            )],
            pending: Vec::new(),
            wts: weights(),
            start_bb,
            prev_pt: None,
            has_portal: false,
            imposed: None,
            total_weight: 0,
        };

        add_children(&mut s, &mut rng, 0);
        while !s.pending.is_empty() {
            let idx = rng.next_i32_bounded(s.pending.len() as i32) as usize;
            let piece_idx = s.pending.remove(idx);
            add_children(&mut s, &mut rng, piece_idx);
        }

        if s.pieces.is_empty() || !s.has_portal {
            continue;
        }

        let (min_y, max_y) = (-64, 63 - 10);
        let mut overall = s.pieces[0].bb;
        for p in &s.pieces[1..] {
            overall = BoundingBox::new(
                IVec3::new(
                    overall.min_x().min(p.bb.min_x()),
                    overall.min_y().min(p.bb.min_y()),
                    overall.min_z().min(p.bb.min_z()),
                ),
                IVec3::new(
                    overall.max_x().max(p.bb.max_x()),
                    overall.max_y().max(p.bb.max_y()),
                    overall.max_z().max(p.bb.max_z()),
                ),
            );
        }
        let mut y1_pos = (overall.max_y() - overall.min_y() + 1) + min_y + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        let dy = y1_pos - overall.max_y();
        return s
            .pieces
            .into_iter()
            .map(|p| StrongholdGeneratedPiece {
                bounding_box: BoundingBox::new(
                    IVec3::new(p.bb.min_x(), p.bb.min_y() + dy, p.bb.min_z()),
                    IVec3::new(p.bb.max_x(), p.bb.max_y() + dy, p.bb.max_z()),
                ),
                orientation: p.dir,
                gen_depth: p.depth,
                data: p.data,
            })
            .collect();
    }
}

/// Registered under `"minecraft:stronghold"`. Biome check at chunk center, surface Y.
pub struct StrongholdStructure;

impl Structure for StrongholdStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        _rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let surface_y = ctx.surface_y();
        let biome = ctx.biome_at(ctx.center_block_x(), surface_y, ctx.center_block_z());
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        Some(GenerationStub {
            position: (ctx.center_block_x(), surface_y, ctx.center_block_z()),
            pieces: generate_pieces(ctx.seed(), ctx.chunk_x(), ctx.chunk_z())
                .into_iter()
                .map(|piece| {
                    let piece_type = piece.data.piece_id();
                    StructurePiece {
                        piece_type: Identifier::new_static("minecraft", piece_type),
                        bounding_box: piece.bounding_box,
                        gen_depth: piece.gen_depth,
                        orientation: Some(piece.orientation),
                        payload: StructurePiecePayload::Procedural(
                            ProceduralPieceData::Stronghold(piece.data),
                        ),
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

    #[test]
    fn orient_box_swaps_offsets_for_east_west_like_vanilla() {
        let foot = IVec3::new(100, 50, 200);
        let off = IVec3::new(-1, -1, 0);
        let size = IVec3::new(5, 5, 7);

        assert_eq!(
            orient_box(foot, off, size, Direction::East),
            BoundingBox::new(IVec3::new(100, 49, 199), IVec3::new(106, 53, 203))
        );
        assert_eq!(
            orient_box(foot, off, size, Direction::West),
            BoundingBox::new(IVec3::new(94, 49, 199), IVec3::new(100, 53, 203))
        );
    }

    #[test]
    fn constructor_rng_state_is_captured_in_piece_payloads() {
        let bb = BoundingBox::new(IVec3::new(0, 20, 0), IVec3::new(13, 30, 14));
        let mut actual = LegacyRandom::from_seed(12_345);
        let mut expected = LegacyRandom::from_seed(12_345);

        let straight = create_piece(PT::Straight, bb, Direction::South, 3, &mut actual);
        let straight_door = StrongholdSmallDoorType::random(&mut expected);
        assert_eq!(
            straight.data,
            StrongholdPieceData::Straight {
                entry_door: straight_door,
                left_child: expected.next_i32_bounded(2) == 0,
                right_child: expected.next_i32_bounded(2) == 0,
            }
        );
        assert_eq!(actual.next_i32(), expected.next_i32());

        let five_crossing = create_piece(PT::FiveCrossing, bb, Direction::South, 4, &mut actual);
        let five_crossing_door = StrongholdSmallDoorType::random(&mut expected);
        assert_eq!(
            five_crossing.data,
            StrongholdPieceData::FiveCrossing {
                entry_door: five_crossing_door,
                left_low: expected.next_bool(),
                left_high: expected.next_bool(),
                right_low: expected.next_bool(),
                right_high: expected.next_i32_bounded(3) > 0,
            }
        );
        assert_eq!(actual.next_i32(), expected.next_i32());

        let room_crossing = create_piece(PT::RoomCrossing, bb, Direction::South, 5, &mut actual);
        let room_crossing_door = StrongholdSmallDoorType::random(&mut expected);
        assert_eq!(
            room_crossing.data,
            StrongholdPieceData::RoomCrossing {
                entry_door: room_crossing_door,
                crossing_type: expected.next_i32_bounded(5),
            }
        );
        assert_eq!(actual.next_i32(), expected.next_i32());
    }

    #[test]
    fn library_and_filler_payloads_capture_non_random_state() {
        let tall_library = create_piece(
            PT::Library,
            BoundingBox::new(IVec3::new(0, 20, 0), IVec3::new(13, 30, 14)),
            Direction::South,
            7,
            &mut LegacyRandom::from_seed(1),
        );
        assert!(matches!(
            tall_library.data,
            StrongholdPieceData::Library { is_tall: true, .. }
        ));

        let short_library = create_piece(
            PT::Library,
            BoundingBox::new(IVec3::new(0, 20, 0), IVec3::new(13, 25, 14)),
            Direction::South,
            7,
            &mut LegacyRandom::from_seed(1),
        );
        assert!(matches!(
            short_library.data,
            StrongholdPieceData::Library { is_tall: false, .. }
        ));

        let filler = create_piece(
            PT::Filler,
            BoundingBox::new(IVec3::new(0, 20, 0), IVec3::new(4, 24, 2)),
            Direction::North,
            4,
            &mut LegacyRandom::from_seed(1),
        );
        assert_eq!(
            filler.data,
            StrongholdPieceData::FillerCorridor { steps: 3 }
        );
    }
}
