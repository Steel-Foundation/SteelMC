//! Nether fortress. Vanilla's `NetherFortressPieces`: start with a `BridgeCrossing`,
//! then weighted BFS over bridge/castle pools honoring place-count, prev-piece, and
//! collision constraints. Structure is vertically offset into `Y ∈ [48, 70]`.

use glam::IVec3;
use steel_registry::structure::StructureData;
use steel_utils::BoundingBox;
use steel_utils::Direction;
use steel_utils::Identifier;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::structure::{
    GenerationStub, ProceduralPieceData, Structure, StructureGenerationContext, StructurePiece,
    StructurePiecePayload,
};

const MAX_DEPTH: i32 = 30;
const LOWEST_Y: i32 = 10;
const MAGIC_START_Y: i32 = 64;
const START_X_OFFSET: i32 = 2;
const START_Z_OFFSET: i32 = 2;
const DIST_LIMIT: i32 = 112;
const Y_LOW_ALLOWED: i32 = 48;
const Y_HIGH_ALLOWED: i32 = 70;

/// Vanilla `Direction.Plane.HORIZONTAL` order: N, E, S, W.
const HORIZONTAL_ORDER: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FortressPieceKind {
    BridgeCrossing,
    BridgeEndFiller,
    BridgeStraight,
    CastleCorridorStairs,
    CastleCorridorTBalcony,
    CastleEntrance,
    CastleSmallCorridorCrossing,
    CastleSmallCorridorLeftTurn,
    CastleSmallCorridor,
    CastleSmallCorridorRightTurn,
    CastleStalkRoom,
    MonsterThrone,
    RoomCrossing,
    StairsRoom,
}

impl FortressPieceKind {
    /// Vanilla's `StructurePieceType` registry path (lowercased, no namespace).
    pub(crate) const fn piece_id(self) -> &'static str {
        match self {
            FortressPieceKind::BridgeCrossing => "nebcr",
            FortressPieceKind::BridgeEndFiller => "nebef",
            FortressPieceKind::BridgeStraight => "nebs",
            FortressPieceKind::CastleCorridorStairs => "neccs",
            FortressPieceKind::CastleCorridorTBalcony => "nectb",
            FortressPieceKind::CastleEntrance => "nece",
            FortressPieceKind::CastleSmallCorridorCrossing => "nescsc",
            FortressPieceKind::CastleSmallCorridorLeftTurn => "nesclt",
            FortressPieceKind::CastleSmallCorridor => "nesc",
            FortressPieceKind::CastleSmallCorridorRightTurn => "nescrt",
            FortressPieceKind::CastleStalkRoom => "necsr",
            FortressPieceKind::MonsterThrone => "nemt",
            FortressPieceKind::RoomCrossing => "nerc",
            FortressPieceKind::StairsRoom => "nesr",
        }
    }

    /// `(offset, size)` for vanilla's `orientBox`.
    const fn geom(self) -> (IVec3, IVec3) {
        match self {
            FortressPieceKind::BridgeCrossing => (IVec3::new(-8, -3, 0), IVec3::new(19, 10, 19)),
            FortressPieceKind::BridgeEndFiller => (IVec3::new(-1, -3, 0), IVec3::new(5, 10, 8)),
            FortressPieceKind::BridgeStraight => (IVec3::new(-1, -3, 0), IVec3::new(5, 10, 19)),
            FortressPieceKind::CastleCorridorStairs => {
                (IVec3::new(-1, -7, 0), IVec3::new(5, 14, 10))
            }
            FortressPieceKind::CastleCorridorTBalcony => {
                (IVec3::new(-3, 0, 0), IVec3::new(9, 7, 9))
            }
            FortressPieceKind::CastleEntrance | FortressPieceKind::CastleStalkRoom => {
                (IVec3::new(-5, -3, 0), IVec3::new(13, 14, 13))
            }
            FortressPieceKind::CastleSmallCorridorCrossing
            | FortressPieceKind::CastleSmallCorridorLeftTurn
            | FortressPieceKind::CastleSmallCorridor
            | FortressPieceKind::CastleSmallCorridorRightTurn => {
                (IVec3::new(-1, 0, 0), IVec3::new(5, 7, 5))
            }
            FortressPieceKind::MonsterThrone => (IVec3::new(-2, 0, 0), IVec3::new(7, 8, 9)),
            FortressPieceKind::RoomCrossing => (IVec3::new(-2, 0, 0), IVec3::new(7, 9, 7)),
            FortressPieceKind::StairsRoom => (IVec3::new(-2, 0, 0), IVec3::new(7, 11, 7)),
        }
    }
}

/// Vanilla nether-fortress piece payload persisted for feature-stage placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FortressPieceData {
    /// Bridge crossing piece.
    BridgeCrossing,
    /// Dead-end bridge filler piece.
    BridgeEndFiller {
        /// Vanilla `BridgeEndFiller.selfSeed`.
        self_seed: i32,
    },
    /// Straight bridge segment.
    BridgeStraight,
    /// Castle corridor stair segment.
    CastleCorridorStairs,
    /// Castle corridor T balcony segment.
    CastleCorridorTBalcony,
    /// Castle entrance room.
    CastleEntrance,
    /// Small castle corridor crossing.
    CastleSmallCorridorCrossing,
    /// Small castle corridor left turn.
    CastleSmallCorridorLeftTurn {
        /// Vanilla `isNeedingChest`.
        is_needing_chest: bool,
    },
    /// Small straight castle corridor.
    CastleSmallCorridor,
    /// Small castle corridor right turn.
    CastleSmallCorridorRightTurn {
        /// Vanilla `isNeedingChest`.
        is_needing_chest: bool,
    },
    /// Nether-wart stair room.
    CastleStalkRoom,
    /// Blaze-spawner throne room.
    MonsterThrone {
        /// Vanilla `hasPlacedSpawner`.
        has_placed_spawner: bool,
    },
    /// Bridge room crossing.
    RoomCrossing,
    /// Bridge stair room.
    StairsRoom,
}

impl FortressPieceData {
    #[must_use]
    pub(crate) const fn kind(self) -> FortressPieceKind {
        match self {
            Self::BridgeCrossing => FortressPieceKind::BridgeCrossing,
            Self::BridgeEndFiller { .. } => FortressPieceKind::BridgeEndFiller,
            Self::BridgeStraight => FortressPieceKind::BridgeStraight,
            Self::CastleCorridorStairs => FortressPieceKind::CastleCorridorStairs,
            Self::CastleCorridorTBalcony => FortressPieceKind::CastleCorridorTBalcony,
            Self::CastleEntrance => FortressPieceKind::CastleEntrance,
            Self::CastleSmallCorridorCrossing => FortressPieceKind::CastleSmallCorridorCrossing,
            Self::CastleSmallCorridorLeftTurn { .. } => {
                FortressPieceKind::CastleSmallCorridorLeftTurn
            }
            Self::CastleSmallCorridor => FortressPieceKind::CastleSmallCorridor,
            Self::CastleSmallCorridorRightTurn { .. } => {
                FortressPieceKind::CastleSmallCorridorRightTurn
            }
            Self::CastleStalkRoom => FortressPieceKind::CastleStalkRoom,
            Self::MonsterThrone { .. } => FortressPieceKind::MonsterThrone,
            Self::RoomCrossing => FortressPieceKind::RoomCrossing,
            Self::StairsRoom => FortressPieceKind::StairsRoom,
        }
    }

    #[must_use]
    pub(crate) const fn piece_id(self) -> &'static str {
        self.kind().piece_id()
    }

    fn new(kind: FortressPieceKind, rng: &mut LegacyRandom) -> Self {
        match kind {
            FortressPieceKind::BridgeCrossing => Self::BridgeCrossing,
            FortressPieceKind::BridgeEndFiller => Self::BridgeEndFiller {
                self_seed: rng.next_i32(),
            },
            FortressPieceKind::BridgeStraight => Self::BridgeStraight,
            FortressPieceKind::CastleCorridorStairs => Self::CastleCorridorStairs,
            FortressPieceKind::CastleCorridorTBalcony => Self::CastleCorridorTBalcony,
            FortressPieceKind::CastleEntrance => Self::CastleEntrance,
            FortressPieceKind::CastleSmallCorridorCrossing => Self::CastleSmallCorridorCrossing,
            FortressPieceKind::CastleSmallCorridorLeftTurn => Self::CastleSmallCorridorLeftTurn {
                is_needing_chest: rng.next_i32_bounded(3) == 0,
            },
            FortressPieceKind::CastleSmallCorridor => Self::CastleSmallCorridor,
            FortressPieceKind::CastleSmallCorridorRightTurn => Self::CastleSmallCorridorRightTurn {
                is_needing_chest: rng.next_i32_bounded(3) == 0,
            },
            FortressPieceKind::CastleStalkRoom => Self::CastleStalkRoom,
            FortressPieceKind::MonsterThrone => Self::MonsterThrone {
                has_placed_spawner: false,
            },
            FortressPieceKind::RoomCrossing => Self::RoomCrossing,
            FortressPieceKind::StairsRoom => Self::StairsRoom,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PieceWeight {
    kind: FortressPieceKind,
    weight: i32,
    max_place_count: i32,
    allow_in_row: bool,
    place_count: i32,
}

impl PieceWeight {
    const fn new(kind: FortressPieceKind, weight: i32, max: i32, allow_in_row: bool) -> Self {
        Self {
            kind,
            weight,
            max_place_count: max,
            allow_in_row,
            place_count: 0,
        }
    }

    const fn do_place(&self) -> bool {
        self.max_place_count == 0 || self.place_count < self.max_place_count
    }
}

fn bridge_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight::new(FortressPieceKind::BridgeStraight, 30, 0, true),
        PieceWeight::new(FortressPieceKind::BridgeCrossing, 10, 4, false),
        PieceWeight::new(FortressPieceKind::RoomCrossing, 10, 4, false),
        PieceWeight::new(FortressPieceKind::StairsRoom, 10, 3, false),
        PieceWeight::new(FortressPieceKind::MonsterThrone, 5, 2, false),
        PieceWeight::new(FortressPieceKind::CastleEntrance, 5, 1, false),
    ]
}

fn castle_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight::new(FortressPieceKind::CastleSmallCorridor, 25, 0, true),
        PieceWeight::new(FortressPieceKind::CastleSmallCorridorCrossing, 15, 5, false),
        PieceWeight::new(
            FortressPieceKind::CastleSmallCorridorRightTurn,
            5,
            10,
            false,
        ),
        PieceWeight::new(FortressPieceKind::CastleSmallCorridorLeftTurn, 5, 10, false),
        PieceWeight::new(FortressPieceKind::CastleCorridorStairs, 10, 3, true),
        PieceWeight::new(FortressPieceKind::CastleCorridorTBalcony, 7, 2, false),
        PieceWeight::new(FortressPieceKind::CastleStalkRoom, 5, 2, false),
    ]
}

/// Output piece record.
#[derive(Debug, Clone, Copy)]
pub struct FortressPiece {
    /// Piece-specific state needed for placement and persistence.
    pub data: FortressPieceData,
    /// World-space bounding box.
    pub bounding_box: BoundingBox,
    /// Piece facing direction.
    pub orientation: Option<Direction>,
    /// Generation depth.
    pub gen_depth: i32,
}

/// Matches `BoundingBox.orientBox`.
fn orient_box(foot: IVec3, off: IVec3, size: IVec3, dir: Direction) -> BoundingBox {
    let s = size - IVec3::ONE;
    match dir {
        Direction::South => BoundingBox::new(foot + off, foot + off + s),
        Direction::North => BoundingBox::new(
            foot + off + IVec3::new(0, 0, -s.z),
            foot + off + IVec3::new(s.x, s.y, 0),
        ),
        Direction::East => {
            let b = foot + IVec3::new(off.z, off.y, off.x);
            BoundingBox::new(b, b + IVec3::new(s.z, s.y, s.x))
        }
        Direction::West => {
            let b = foot + IVec3::new(off.z, off.y, off.x);
            let rs = IVec3::new(s.z, s.y, s.x);
            BoundingBox::new(b - IVec3::new(rs.x, 0, 0), b + IVec3::new(0, rs.y, rs.z))
        }
        _ => unreachable!("orient_box non-horizontal direction"),
    }
}

/// Matches `StructurePiece.makeBoundingBox`: width rotates with the direction axis.
fn make_bounding_box(
    pos: IVec3,
    dir: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    let size = match dir {
        Direction::North | Direction::South => IVec3::new(width, height, depth),
        Direction::East | Direction::West => IVec3::new(depth, height, width),
        _ => unreachable!(),
    };
    BoundingBox::new(pos, pos + size - IVec3::ONE)
}

const fn is_ok_box(bb: &BoundingBox) -> bool {
    bb.min_y() > LOWEST_Y
}

fn find_collision<'a>(pieces: &'a [FortressPiece], bb: &BoundingBox) -> Option<&'a FortressPiece> {
    pieces.iter().find(|p| p.bounding_box.intersects(*bb))
}

struct Builder {
    pieces: Vec<FortressPiece>,
    pending: Vec<FortressPiece>,
    start_bb_min: IVec3,
    bridge_weights: Vec<PieceWeight>,
    castle_weights: Vec<PieceWeight>,
    previous_kind: Option<FortressPieceKind>,
}

impl Builder {
    fn add_and_enqueue(&mut self, piece: FortressPiece) {
        self.pieces.push(piece);
        self.pending.push(piece);
    }
}

/// Mirrors vanilla's `findAndCreateBridgePieceFactory` + `PIECE.createPiece`.
fn create_piece(
    kind: FortressPieceKind,
    pieces: &[FortressPiece],
    rng: &mut LegacyRandom,
    foot: IVec3,
    dir: Direction,
    gen_depth: i32,
) -> Option<FortressPiece> {
    let (off, size) = kind.geom();
    let bb = orient_box(foot, off, size, dir);
    if !is_ok_box(&bb) || find_collision(pieces, &bb).is_some() {
        return None;
    }
    Some(FortressPiece {
        data: FortressPieceData::new(kind, rng),
        bounding_box: bb,
        orientation: Some(dir),
        gen_depth,
    })
}

/// Vanilla's `generatePiece`. Falls back to `BridgeEndFiller` if no weighted pick
/// succeeds within 5 attempts. On ineligible picks vanilla falls through to
/// subsequent pieces in the list.
fn generate_piece_weighted(
    is_castle: bool,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    foot: IVec3,
    dir: Direction,
    depth: i32,
) -> Option<FortressPiece> {
    let total_weight: i32 = {
        let pool = if is_castle {
            &builder.castle_weights
        } else {
            &builder.bridge_weights
        };
        let has_any = pool
            .iter()
            .any(|p| p.max_place_count > 0 && p.place_count < p.max_place_count);
        let sum: i32 = pool.iter().map(|p| p.weight).sum();
        if has_any { sum } else { -1 }
    };

    if total_weight > 0 && depth <= MAX_DEPTH {
        for _ in 0..5 {
            let mut choice = rng.next_i32_bounded(total_weight);
            let mut i = 0;
            loop {
                let (kind, allow_in_row, do_place) = {
                    let pool = if is_castle {
                        &builder.castle_weights
                    } else {
                        &builder.bridge_weights
                    };
                    if i >= pool.len() {
                        break;
                    }
                    choice -= pool[i].weight;
                    (pool[i].kind, pool[i].allow_in_row, pool[i].do_place())
                };
                if choice >= 0 {
                    i += 1;
                    continue;
                }
                if !do_place || (Some(kind) == builder.previous_kind && !allow_in_row) {
                    break;
                }
                if let Some(p) = create_piece(kind, &builder.pieces, rng, foot, dir, depth) {
                    let pool = if is_castle {
                        &mut builder.castle_weights
                    } else {
                        &mut builder.bridge_weights
                    };
                    pool[i].place_count += 1;
                    builder.previous_kind = Some(kind);
                    if !pool[i].do_place() {
                        pool.remove(i);
                    }
                    return Some(p);
                }
                i += 1;
            }
        }
    }

    create_piece(
        FortressPieceKind::BridgeEndFiller,
        &builder.pieces,
        rng,
        foot,
        dir,
        depth,
    )
}

/// Out-of-range branch builds a `BridgeEndFiller` (consuming RNG for `selfSeed`)
/// then discards it. We mirror vanilla: call `create_piece` for RNG sync, don't add.
fn generate_and_add_piece(
    is_castle: bool,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    foot: IVec3,
    dir: Direction,
    depth: i32,
) {
    if (foot.x - builder.start_bb_min.x).abs() > DIST_LIMIT
        || (foot.z - builder.start_bb_min.z).abs() > DIST_LIMIT
    {
        let _ = create_piece(
            FortressPieceKind::BridgeEndFiller,
            &builder.pieces,
            rng,
            foot,
            dir,
            depth,
        );
        return;
    }
    if let Some(piece) = generate_piece_weighted(is_castle, builder, rng, foot, dir, depth + 1) {
        builder.add_and_enqueue(piece);
    }
}

/// Parent context threaded through `generate_child_*`.
#[derive(Clone, Copy)]
struct ParentRef {
    bb: BoundingBox,
    orientation: Direction,
    gen_depth: i32,
}

fn generate_child_forward(
    parent: ParentRef,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    x_off: i32,
    y_off: i32,
    is_castle: bool,
) {
    let bb = parent.bb;
    let y = bb.min_y() + y_off;
    let foot = match parent.orientation {
        Direction::North => IVec3::new(bb.min_x() + x_off, y, bb.min_z() - 1),
        Direction::South => IVec3::new(bb.min_x() + x_off, y, bb.max_z() + 1),
        Direction::West => IVec3::new(bb.min_x() - 1, y, bb.min_z() + x_off),
        Direction::East => IVec3::new(bb.max_x() + 1, y, bb.min_z() + x_off),
        _ => return,
    };
    generate_and_add_piece(
        is_castle,
        builder,
        rng,
        foot,
        parent.orientation,
        parent.gen_depth,
    );
}

fn generate_child_left(
    parent: ParentRef,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    y_off: i32,
    z_off: i32,
    is_castle: bool,
) {
    let bb = parent.bb;
    let (foot, dir) = match parent.orientation {
        Direction::North | Direction::South => (
            IVec3::new(bb.min_x() - 1, bb.min_y() + y_off, bb.min_z() + z_off),
            Direction::West,
        ),
        Direction::West | Direction::East => (
            IVec3::new(bb.min_x() + z_off, bb.min_y() + y_off, bb.min_z() - 1),
            Direction::North,
        ),
        _ => return,
    };
    generate_and_add_piece(is_castle, builder, rng, foot, dir, parent.gen_depth);
}

fn generate_child_right(
    parent: ParentRef,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    y_off: i32,
    z_off: i32,
    is_castle: bool,
) {
    let bb = parent.bb;
    let (foot, dir) = match parent.orientation {
        Direction::North | Direction::South => (
            IVec3::new(bb.max_x() + 1, bb.min_y() + y_off, bb.min_z() + z_off),
            Direction::East,
        ),
        Direction::West | Direction::East => (
            IVec3::new(bb.min_x() + z_off, bb.min_y() + y_off, bb.max_z() + 1),
            Direction::South,
        ),
        _ => return,
    };
    generate_and_add_piece(is_castle, builder, rng, foot, dir, parent.gen_depth);
}

fn add_children(piece: FortressPiece, builder: &mut Builder, rng: &mut LegacyRandom) {
    let Some(orientation) = piece.orientation else {
        return;
    };
    let parent = ParentRef {
        bb: piece.bounding_box,
        orientation,
        gen_depth: piece.gen_depth,
    };
    match piece.data.kind() {
        FortressPieceKind::BridgeCrossing => {
            generate_child_forward(parent, builder, rng, 8, 3, false);
            generate_child_left(parent, builder, rng, 3, 8, false);
            generate_child_right(parent, builder, rng, 3, 8, false);
        }
        FortressPieceKind::BridgeStraight => {
            generate_child_forward(parent, builder, rng, 1, 3, false);
        }
        FortressPieceKind::CastleCorridorStairs | FortressPieceKind::CastleSmallCorridor => {
            generate_child_forward(parent, builder, rng, 1, 0, true);
        }
        FortressPieceKind::CastleCorridorTBalcony => {
            let z_off = match orientation {
                Direction::West | Direction::North => 5,
                _ => 1,
            };
            let l = rng.next_i32_bounded(8) > 0;
            generate_child_left(parent, builder, rng, 0, z_off, l);
            let r = rng.next_i32_bounded(8) > 0;
            generate_child_right(parent, builder, rng, 0, z_off, r);
        }
        FortressPieceKind::CastleEntrance => {
            generate_child_forward(parent, builder, rng, 5, 3, true);
        }
        FortressPieceKind::CastleSmallCorridorCrossing => {
            generate_child_forward(parent, builder, rng, 1, 0, true);
            generate_child_left(parent, builder, rng, 0, 1, true);
            generate_child_right(parent, builder, rng, 0, 1, true);
        }
        FortressPieceKind::CastleSmallCorridorLeftTurn => {
            generate_child_left(parent, builder, rng, 0, 1, true);
        }
        FortressPieceKind::CastleSmallCorridorRightTurn => {
            generate_child_right(parent, builder, rng, 0, 1, true);
        }
        FortressPieceKind::CastleStalkRoom => {
            generate_child_forward(parent, builder, rng, 5, 3, true);
            generate_child_forward(parent, builder, rng, 5, 11, true);
        }
        FortressPieceKind::RoomCrossing => {
            generate_child_forward(parent, builder, rng, 2, 0, false);
            generate_child_left(parent, builder, rng, 0, 2, false);
            generate_child_right(parent, builder, rng, 0, 2, false);
        }
        FortressPieceKind::StairsRoom => {
            generate_child_right(parent, builder, rng, 6, 2, false);
        }
        // MonsterThrone, BridgeEndFiller: leaves.
        FortressPieceKind::MonsterThrone | FortressPieceKind::BridgeEndFiller => {}
    }
}

fn overall_bb(pieces: &[FortressPiece]) -> BoundingBox {
    let mut bb = pieces[0].bounding_box;
    for p in &pieces[1..] {
        bb = BoundingBox::encapsulating(&bb, &p.bounding_box);
    }
    bb
}

fn move_inside_heights(
    pieces: &mut [FortressPiece],
    rng: &mut LegacyRandom,
    lowest_allowed: i32,
    highest_allowed: i32,
) {
    if pieces.is_empty() {
        return;
    }
    let bb = overall_bb(pieces);
    let height_span = highest_allowed - lowest_allowed + 1 - bb.height();
    let y0 = if height_span > 1 {
        lowest_allowed + rng.next_i32_bounded(height_span)
    } else {
        lowest_allowed
    };
    let dy = y0 - bb.min_y();
    if dy == 0 {
        return;
    }
    let delta = IVec3::new(0, dy, 0);
    for p in pieces {
        p.bounding_box = p.bounding_box.translate(delta);
    }
}

/// All fortress pieces for the chunk, vertically offset into `Y ∈ [48, 70]`.
pub fn generate_fortress_pieces(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
) -> Vec<FortressPiece> {
    let start_dir = HORIZONTAL_ORDER[rng.next_i32_bounded(4) as usize];
    let west = (chunk_x << 4) + START_X_OFFSET;
    let north = (chunk_z << 4) + START_Z_OFFSET;
    let start_bb = make_bounding_box(
        IVec3::new(west, MAGIC_START_Y, north),
        start_dir,
        19,
        10,
        19,
    );
    let start_piece = FortressPiece {
        data: FortressPieceData::BridgeCrossing,
        bounding_box: start_bb,
        orientation: Some(start_dir),
        gen_depth: 0,
    };

    let mut builder = Builder {
        pieces: vec![start_piece],
        pending: Vec::new(),
        start_bb_min: start_bb.min_corner(),
        bridge_weights: bridge_weights(),
        castle_weights: castle_weights(),
        previous_kind: None,
    };

    add_children(start_piece, &mut builder, rng);
    while !builder.pending.is_empty() {
        let pos = rng.next_i32_bounded(builder.pending.len() as i32) as usize;
        let pending = builder.pending.remove(pos);
        add_children(pending, &mut builder, rng);
    }

    move_inside_heights(&mut builder.pieces, rng, Y_LOW_ALLOWED, Y_HIGH_ALLOWED);
    builder.pieces
}

/// Registered under `"minecraft:fortress"`. Shares the `nether_complexes` set with
/// `bastion_remnant` (jigsaw), so it's dispatched from the jigsaw arm's fallthrough.
pub struct NetherFortressStructure;

impl Structure for NetherFortressStructure {
    fn find_generation_point(
        &self,
        ctx: &mut dyn StructureGenerationContext,
        structure: &StructureData,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        // Biome check at (chunkMinX, 64, chunkMinZ) per vanilla.
        let (biome_x, biome_z) = (ctx.chunk_min_x(), ctx.chunk_min_z());
        let biome = ctx.biome_at(biome_x, 64, biome_z);
        if !structure.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let pieces_out = generate_fortress_pieces(ctx.chunk_x(), ctx.chunk_z(), rng);
        if pieces_out.is_empty() {
            return None;
        }

        Some(GenerationStub {
            position: (biome_x, 64, biome_z),
            pieces: pieces_out
                .into_iter()
                .map(|p| StructurePiece {
                    piece_type: Identifier::new_static("minecraft", p.data.piece_id()),
                    bounding_box: p.bounding_box,
                    gen_depth: p.gen_depth,
                    orientation: p.orientation,
                    payload: StructurePiecePayload::Procedural(
                        ProceduralPieceData::NetherFortress(p.data),
                    ),
                    ground_level_delta: 0,
                    junctions: Vec::new(),
                    projection: None,
                })
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fortress_constructor_rng_state_is_captured_in_piece_payloads() {
        let mut expected = LegacyRandom::from_seed(1234);
        let expected_self_seed = expected.next_i32();
        let mut rng = LegacyRandom::from_seed(1234);
        let filler = create_piece(
            FortressPieceKind::BridgeEndFiller,
            &[],
            &mut rng,
            IVec3::new(0, 64, 0),
            Direction::South,
            1,
        )
        .expect("bridge end filler should fit");
        assert_eq!(
            filler.data,
            FortressPieceData::BridgeEndFiller {
                self_seed: expected_self_seed,
            }
        );
        assert_eq!(rng.next_i32(), expected.next_i32());

        let mut expected = LegacyRandom::from_seed(5678);
        let expected_needs_chest = expected.next_i32_bounded(3) == 0;
        let mut rng = LegacyRandom::from_seed(5678);
        let turn = create_piece(
            FortressPieceKind::CastleSmallCorridorLeftTurn,
            &[],
            &mut rng,
            IVec3::new(0, 64, 0),
            Direction::South,
            1,
        )
        .expect("small corridor turn should fit");
        assert_eq!(
            turn.data,
            FortressPieceData::CastleSmallCorridorLeftTurn {
                is_needing_chest: expected_needs_chest,
            }
        );
        assert_eq!(rng.next_i32(), expected.next_i32());
    }
}
