//! Nether fortress piece generation.
//!
//! Ports vanilla's `NetherFortressPieces`. A `StartPiece` (`BridgeCrossing`) is
//! placed first, then a weighted BFS-like process picks pieces from either
//! the bridge or castle pool, honoring placement limits, previous-piece
//! constraints, and collision against already placed pieces. After all
//! children are resolved the entire structure is randomly offset vertically
//! into `Y ∈ [48, 70]` via `moveInsideHeights`.

use steel_utils::BoundingBox;
use steel_utils::Direction;
use steel_utils::Identifier;
use steel_utils::density::DimensionNoises;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{GenerationContext, GenerationStub, Structure, StructurePiece};

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
enum PieceKind {
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

impl PieceKind {
    /// Returns the vanilla identifier path (without `minecraft:` prefix).
    pub const fn piece_id(self) -> &'static str {
        match self {
            // Match vanilla's `StructurePieceType` registry (lowercased).
            PieceKind::BridgeCrossing => "nebcr",
            PieceKind::BridgeEndFiller => "nebef",
            PieceKind::BridgeStraight => "nebs",
            PieceKind::CastleCorridorStairs => "neccs",
            PieceKind::CastleCorridorTBalcony => "nectb",
            PieceKind::CastleEntrance => "nece",
            PieceKind::CastleSmallCorridorCrossing => "nescsc",
            PieceKind::CastleSmallCorridorLeftTurn => "nesclt",
            PieceKind::CastleSmallCorridor => "nesc",
            PieceKind::CastleSmallCorridorRightTurn => "nescrt",
            PieceKind::CastleStalkRoom => "necsr",
            PieceKind::MonsterThrone => "nemt",
            PieceKind::RoomCrossing => "nerc",
            PieceKind::StairsRoom => "nesr",
        }
    }

    /// Returns `(offX, offY, offZ, width, height, depth)` for vanilla's `orientBox`.
    const fn geom(self) -> (i32, i32, i32, i32, i32, i32) {
        match self {
            PieceKind::BridgeCrossing => (-8, -3, 0, 19, 10, 19),
            PieceKind::BridgeEndFiller => (-1, -3, 0, 5, 10, 8),
            PieceKind::BridgeStraight => (-1, -3, 0, 5, 10, 19),
            PieceKind::CastleCorridorStairs => (-1, -7, 0, 5, 14, 10),
            PieceKind::CastleCorridorTBalcony => (-3, 0, 0, 9, 7, 9),
            PieceKind::CastleEntrance | PieceKind::CastleStalkRoom => (-5, -3, 0, 13, 14, 13),
            PieceKind::CastleSmallCorridorCrossing
            | PieceKind::CastleSmallCorridorLeftTurn
            | PieceKind::CastleSmallCorridor
            | PieceKind::CastleSmallCorridorRightTurn => (-1, 0, 0, 5, 7, 5),
            PieceKind::MonsterThrone => (-2, 0, 0, 7, 8, 9),
            PieceKind::RoomCrossing => (-2, 0, 0, 7, 9, 7),
            PieceKind::StairsRoom => (-2, 0, 0, 7, 11, 7),
        }
    }

    /// Whether the piece's constructor consumes RNG after a successful
    /// collision check.
    #[expect(
        clippy::match_same_arms,
        reason = "arms split by distinct vanilla RNG semantics (selfSeed vs chest-gate)"
    )]
    const fn extra_rng_in_ctor(self) -> u32 {
        match self {
            // BridgeEndFiller consumes `random.nextInt()` (unbounded) for selfSeed.
            PieceKind::BridgeEndFiller => 1,
            // Left/right small-corridor turns consume `random.nextInt(3)` for isNeedingChest.
            PieceKind::CastleSmallCorridorLeftTurn | PieceKind::CastleSmallCorridorRightTurn => 1,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PieceWeight {
    kind: PieceKind,
    weight: i32,
    max_place_count: i32,
    allow_in_row: bool,
    place_count: i32,
}

impl PieceWeight {
    const fn new(kind: PieceKind, weight: i32, max: i32, allow_in_row: bool) -> Self {
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
        PieceWeight::new(PieceKind::BridgeStraight, 30, 0, true),
        PieceWeight::new(PieceKind::BridgeCrossing, 10, 4, false),
        PieceWeight::new(PieceKind::RoomCrossing, 10, 4, false),
        PieceWeight::new(PieceKind::StairsRoom, 10, 3, false),
        PieceWeight::new(PieceKind::MonsterThrone, 5, 2, false),
        PieceWeight::new(PieceKind::CastleEntrance, 5, 1, false),
    ]
}

fn castle_weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight::new(PieceKind::CastleSmallCorridor, 25, 0, true),
        PieceWeight::new(PieceKind::CastleSmallCorridorCrossing, 15, 5, false),
        PieceWeight::new(PieceKind::CastleSmallCorridorRightTurn, 5, 10, false),
        PieceWeight::new(PieceKind::CastleSmallCorridorLeftTurn, 5, 10, false),
        PieceWeight::new(PieceKind::CastleCorridorStairs, 10, 3, true),
        PieceWeight::new(PieceKind::CastleCorridorTBalcony, 7, 2, false),
        PieceWeight::new(PieceKind::CastleStalkRoom, 5, 2, false),
    ]
}

/// Output piece record.
#[derive(Debug, Clone, Copy)]
pub struct FortressPiece {
    /// Short identifier path (under `minecraft:`) for this piece type.
    pub kind_id: &'static str,
    /// World-space bounding box.
    pub bounding_box: BoundingBox,
    /// Piece facing direction.
    pub orientation: Option<Direction>,
    /// Generation depth.
    pub gen_depth: i32,
}

fn orient_box(
    foot: (i32, i32, i32),
    off: (i32, i32, i32),
    size: (i32, i32, i32),
    dir: Direction,
) -> BoundingBox {
    let (foot_x, foot_y, foot_z) = foot;
    let (off_x, off_y, off_z) = off;
    let (width, height, depth) = size;
    // Matches `BoundingBox.orientBox`.
    match dir {
        Direction::South => BoundingBox::new(
            foot_x + off_x,
            foot_y + off_y,
            foot_z + off_z,
            foot_x + width - 1 + off_x,
            foot_y + height - 1 + off_y,
            foot_z + depth - 1 + off_z,
        ),
        Direction::North => BoundingBox::new(
            foot_x + off_x,
            foot_y + off_y,
            foot_z - depth + 1 + off_z,
            foot_x + width - 1 + off_x,
            foot_y + height - 1 + off_y,
            foot_z + off_z,
        ),
        Direction::West => BoundingBox::new(
            foot_x - depth + 1 + off_z,
            foot_y + off_y,
            foot_z + off_x,
            foot_x + off_z,
            foot_y + height - 1 + off_y,
            foot_z + width - 1 + off_x,
        ),
        Direction::East => BoundingBox::new(
            foot_x + off_z,
            foot_y + off_y,
            foot_z + off_x,
            foot_x + depth - 1 + off_z,
            foot_y + height - 1 + off_y,
            foot_z + width - 1 + off_x,
        ),
        _ => unreachable!("orient_box called with non-horizontal direction"),
    }
}

fn make_bounding_box(
    x: i32,
    y: i32,
    z: i32,
    dir: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    // Matches `StructurePiece.makeBoundingBox`: width rotates with direction axis.
    match dir {
        Direction::North | Direction::South => {
            BoundingBox::new(x, y, z, x + width - 1, y + height - 1, z + depth - 1)
        }
        Direction::East | Direction::West => {
            BoundingBox::new(x, y, z, x + depth - 1, y + height - 1, z + width - 1)
        }
        _ => unreachable!(),
    }
}

const fn is_ok_box(bb: &BoundingBox) -> bool {
    bb.min_y > LOWEST_Y
}

fn find_collision<'a>(pieces: &'a [FortressPiece], bb: &BoundingBox) -> Option<&'a FortressPiece> {
    pieces.iter().find(|p| p.bounding_box.intersects(bb))
}

/// Record pushed onto the pending queue.
#[derive(Debug, Clone, Copy)]
struct Pending {
    piece: FortressPiece,
}

struct Builder {
    pieces: Vec<FortressPiece>,
    pending: Vec<Pending>,
    start_bb_min_x: i32,
    start_bb_min_z: i32,
    bridge_weights: Vec<PieceWeight>,
    castle_weights: Vec<PieceWeight>,
    /// Index (by kind) of the last-placed weighted piece for in-row checks.
    previous_kind: Option<PieceKind>,
}

impl Builder {
    fn add_and_enqueue(&mut self, piece: FortressPiece) {
        self.pieces.push(piece);
        self.pending.push(Pending { piece });
    }
}

/// Creates a single piece given its kind at `foot` facing `dir`.
/// Mirrors vanilla's `findAndCreateBridgePieceFactory` + `PIECE.createPiece`.
/// Returns the built piece if the box passes `isOkBox` and has no collision.
fn create_piece(
    kind: PieceKind,
    pieces: &[FortressPiece],
    rng: &mut LegacyRandom,
    foot: (i32, i32, i32),
    dir: Direction,
    gen_depth: i32,
) -> Option<FortressPiece> {
    let (ox, oy, oz, w, h, d) = kind.geom();
    let bb = orient_box(foot, (ox, oy, oz), (w, h, d), dir);
    if !is_ok_box(&bb) || find_collision(pieces, &bb).is_some() {
        return None;
    }

    // Constructor-side RNG consumption.
    for _ in 0..kind.extra_rng_in_ctor() {
        if matches!(kind, PieceKind::BridgeEndFiller) {
            // selfSeed = random.nextInt() (unbounded 32-bit)
            let _ = rng.next_i32();
        } else {
            // isNeedingChest = random.nextInt(3) == 0
            let _ = rng.next_i32_bounded(3);
        }
    }

    Some(FortressPiece {
        kind_id: kind.piece_id(),
        bounding_box: bb,
        orientation: Some(dir),
        gen_depth,
    })
}

/// Weighted piece selection matching vanilla's `generatePiece`. Always
/// returns a piece — falls back to `BridgeEndFiller` if no weighted pick
/// succeeds within 5 attempts.
fn generate_piece_weighted(
    is_castle: bool,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    foot: (i32, i32, i32),
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
    let do_stuff = total_weight > 0 && depth <= MAX_DEPTH;

    if do_stuff {
        for _ in 0..5 {
            let mut weight_selection = rng.next_i32_bounded(total_weight);

            let mut iter_idx = 0;
            loop {
                let (piece_kind, piece_weight, piece_allow_in_row, piece_do_place) = {
                    let pool = if is_castle {
                        &builder.castle_weights
                    } else {
                        &builder.bridge_weights
                    };
                    if iter_idx >= pool.len() {
                        break;
                    }
                    let p = &pool[iter_idx];
                    (p.kind, p.weight, p.allow_in_row, p.do_place())
                };
                weight_selection -= piece_weight;
                if weight_selection >= 0 {
                    iter_idx += 1;
                    continue;
                }
                // Hit a candidate.
                if !piece_do_place
                    || (Some(piece_kind) == builder.previous_kind && !piece_allow_in_row)
                {
                    break;
                }

                let made = create_piece(piece_kind, &builder.pieces, rng, foot, dir, depth);
                if let Some(p) = made {
                    // Update counters.
                    let pool = if is_castle {
                        &mut builder.castle_weights
                    } else {
                        &mut builder.bridge_weights
                    };
                    pool[iter_idx].place_count += 1;
                    builder.previous_kind = Some(piece_kind);
                    if !pool[iter_idx].do_place() {
                        pool.remove(iter_idx);
                    }
                    return Some(p);
                }
                // Collision or invalid box — vanilla falls through and tries
                // subsequent pieces in the for loop.
                iter_idx += 1;
            }
        }
    }

    // Fallback — BridgeEndFiller leaf piece.
    create_piece(
        PieceKind::BridgeEndFiller,
        &builder.pieces,
        rng,
        foot,
        dir,
        depth,
    )
}

fn generate_and_add_piece(
    is_castle: bool,
    builder: &mut Builder,
    rng: &mut LegacyRandom,
    foot: (i32, i32, i32),
    dir: Direction,
    depth: i32,
) {
    // Distance cap from the start piece. Vanilla's out-of-range branch builds
    // a BridgeEndFiller (consuming RNG for its selfSeed) but then DISCARDS it
    // — `generateAndAddPiece` returns the piece without calling `addPiece`,
    // and the caller (`generateChildX` → `addChildren`) throws the result away.
    // We mirror that: still call create_piece so RNG stays in sync, but don't
    // add the piece to the placed set.
    if (foot.0 - builder.start_bb_min_x).abs() > DIST_LIMIT
        || (foot.2 - builder.start_bb_min_z).abs() > DIST_LIMIT
    {
        let _ = create_piece(
            PieceKind::BridgeEndFiller,
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

/// Parent-piece context used by the `generate_child_*` helpers. Groups the
/// bounding box, orientation, and generation depth threaded through vanilla's
/// `generateChildForward/Left/Right` call surface.
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
    let (fx, fy, fz, dir) = match parent.orientation {
        Direction::North => (
            bb.min_x + x_off,
            bb.min_y + y_off,
            bb.min_z - 1,
            Direction::North,
        ),
        Direction::South => (
            bb.min_x + x_off,
            bb.min_y + y_off,
            bb.max_z + 1,
            Direction::South,
        ),
        Direction::West => (
            bb.min_x - 1,
            bb.min_y + y_off,
            bb.min_z + x_off,
            Direction::West,
        ),
        Direction::East => (
            bb.max_x + 1,
            bb.min_y + y_off,
            bb.min_z + x_off,
            Direction::East,
        ),
        _ => return,
    };
    generate_and_add_piece(is_castle, builder, rng, (fx, fy, fz), dir, parent.gen_depth);
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
    let (fx, fy, fz, dir) = match parent.orientation {
        Direction::North | Direction::South => (
            bb.min_x - 1,
            bb.min_y + y_off,
            bb.min_z + z_off,
            Direction::West,
        ),
        Direction::West | Direction::East => (
            bb.min_x + z_off,
            bb.min_y + y_off,
            bb.min_z - 1,
            Direction::North,
        ),
        _ => return,
    };
    generate_and_add_piece(is_castle, builder, rng, (fx, fy, fz), dir, parent.gen_depth);
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
    let (fx, fy, fz, dir) = match parent.orientation {
        Direction::North | Direction::South => (
            bb.max_x + 1,
            bb.min_y + y_off,
            bb.min_z + z_off,
            Direction::East,
        ),
        Direction::West | Direction::East => (
            bb.min_x + z_off,
            bb.min_y + y_off,
            bb.max_z + 1,
            Direction::South,
        ),
        _ => return,
    };
    generate_and_add_piece(is_castle, builder, rng, (fx, fy, fz), dir, parent.gen_depth);
}

fn add_children(piece: FortressPiece, builder: &mut Builder, rng: &mut LegacyRandom) {
    let Some(orientation) = piece.orientation else {
        return;
    };
    let bb = piece.bounding_box;
    let gd = piece.gen_depth;

    let parent = ParentRef {
        bb,
        orientation,
        gen_depth: gd,
    };
    match piece.kind_id {
        // BridgeCrossing (also the start piece).
        id if id == PieceKind::BridgeCrossing.piece_id() => {
            generate_child_forward(parent, builder, rng, 8, 3, false);
            generate_child_left(parent, builder, rng, 3, 8, false);
            generate_child_right(parent, builder, rng, 3, 8, false);
        }
        id if id == PieceKind::BridgeStraight.piece_id() => {
            generate_child_forward(parent, builder, rng, 1, 3, false);
        }
        id if id == PieceKind::CastleCorridorStairs.piece_id() => {
            generate_child_forward(parent, builder, rng, 1, 0, true);
        }
        id if id == PieceKind::CastleCorridorTBalcony.piece_id() => {
            let z_off = match orientation {
                Direction::West | Direction::North => 5,
                _ => 1,
            };
            let left_is_castle = rng.next_i32_bounded(8) > 0;
            generate_child_left(parent, builder, rng, 0, z_off, left_is_castle);
            let right_is_castle = rng.next_i32_bounded(8) > 0;
            generate_child_right(parent, builder, rng, 0, z_off, right_is_castle);
        }
        id if id == PieceKind::CastleEntrance.piece_id() => {
            generate_child_forward(parent, builder, rng, 5, 3, true);
        }
        id if id == PieceKind::CastleSmallCorridorCrossing.piece_id() => {
            generate_child_forward(parent, builder, rng, 1, 0, true);
            generate_child_left(parent, builder, rng, 0, 1, true);
            generate_child_right(parent, builder, rng, 0, 1, true);
        }
        id if id == PieceKind::CastleSmallCorridorLeftTurn.piece_id() => {
            generate_child_left(parent, builder, rng, 0, 1, true);
        }
        id if id == PieceKind::CastleSmallCorridor.piece_id() => {
            generate_child_forward(parent, builder, rng, 1, 0, true);
        }
        id if id == PieceKind::CastleSmallCorridorRightTurn.piece_id() => {
            generate_child_right(parent, builder, rng, 0, 1, true);
        }
        id if id == PieceKind::CastleStalkRoom.piece_id() => {
            generate_child_forward(parent, builder, rng, 5, 3, true);
            generate_child_forward(parent, builder, rng, 5, 11, true);
        }
        id if id == PieceKind::RoomCrossing.piece_id() => {
            generate_child_forward(parent, builder, rng, 2, 0, false);
            generate_child_left(parent, builder, rng, 0, 2, false);
            generate_child_right(parent, builder, rng, 0, 2, false);
        }
        id if id == PieceKind::StairsRoom.piece_id() => {
            generate_child_right(parent, builder, rng, 6, 2, false);
        }
        // Leaf pieces (MonsterThrone, BridgeEndFiller): no children.
        _ => {}
    }
}

fn offset_vertically(pieces: &mut [FortressPiece], dy: i32) {
    if dy == 0 {
        return;
    }
    for p in pieces {
        p.bounding_box = BoundingBox::new(
            p.bounding_box.min_x,
            p.bounding_box.min_y + dy,
            p.bounding_box.min_z,
            p.bounding_box.max_x,
            p.bounding_box.max_y + dy,
            p.bounding_box.max_z,
        );
    }
}

fn overall_bb(pieces: &[FortressPiece]) -> BoundingBox {
    let mut bb = pieces[0].bounding_box;
    for p in &pieces[1..] {
        bb = BoundingBox::new(
            bb.min_x.min(p.bounding_box.min_x),
            bb.min_y.min(p.bounding_box.min_y),
            bb.min_z.min(p.bounding_box.min_z),
            bb.max_x.max(p.bounding_box.max_x),
            bb.max_y.max(p.bounding_box.max_y),
            bb.max_z.max(p.bounding_box.max_z),
        );
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
    let y_span = bb.max_y - bb.min_y + 1;
    let height_span = highest_allowed - lowest_allowed + 1 - y_span;
    let y0 = if height_span > 1 {
        lowest_allowed + rng.next_i32_bounded(height_span)
    } else {
        lowest_allowed
    };
    let dy = y0 - bb.min_y;
    offset_vertically(pieces, dy);
}

/// Generates all fortress pieces for the chunk. The returned pieces are
/// vertically offset to sit within `Y ∈ [48, 70]` per vanilla.
pub fn generate_fortress_pieces(
    chunk_x: i32,
    chunk_z: i32,
    rng: &mut LegacyRandom,
) -> Vec<FortressPiece> {
    // StartPiece constructor: super(getRandomHorizontalDirection(random), west=2, north=2)
    let start_dir = HORIZONTAL_ORDER[rng.next_i32_bounded(4) as usize];
    let west = (chunk_x << 4) + START_X_OFFSET;
    let north = (chunk_z << 4) + START_Z_OFFSET;
    let start_bb = make_bounding_box(west, MAGIC_START_Y, north, start_dir, 19, 10, 19);

    let start_piece = FortressPiece {
        kind_id: PieceKind::BridgeCrossing.piece_id(),
        bounding_box: start_bb,
        orientation: Some(start_dir),
        gen_depth: 0,
    };

    let mut builder = Builder {
        pieces: vec![start_piece],
        pending: Vec::new(),
        start_bb_min_x: start_bb.min_x,
        start_bb_min_z: start_bb.min_z,
        bridge_weights: bridge_weights(),
        castle_weights: castle_weights(),
        previous_kind: None,
    };

    // StartPiece.addChildren (= BridgeCrossing.addChildren)
    add_children(start_piece, &mut builder, rng);

    // Pending queue: pick random entries until empty.
    while !builder.pending.is_empty() {
        let pos = rng.next_i32_bounded(builder.pending.len() as i32) as usize;
        let pending = builder.pending.remove(pos);
        add_children(pending.piece, &mut builder, rng);
    }

    // Vertical adjustment.
    move_inside_heights(&mut builder.pieces, rng, Y_LOW_ALLOWED, Y_HIGH_ALLOWED);
    builder.pieces
}

/// `Structure` impl — registered under `"minecraft:fortress"`. Shares the
/// `nether_complexes` set with `bastion_remnant` (a jigsaw entry), so it's
/// dispatched from the jigsaw arm's non-jigsaw fallthrough.
pub struct NetherFortressStructure;

impl<N: DimensionNoises> Structure<N> for NetherFortressStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        // Biome check at (chunkMinX, 64, chunkMinZ) per vanilla.
        let biome_x = ctx.chunk_min_x;
        let biome_z = ctx.chunk_min_z;
        let biome = ctx.biome_at(biome_x, 64, biome_z);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let pieces_out = generate_fortress_pieces(ctx.chunk_x, ctx.chunk_z, rng);
        if pieces_out.is_empty() {
            return None;
        }

        let pieces = pieces_out
            .into_iter()
            .map(|p| StructurePiece {
                piece_type: Identifier::new_static("minecraft", p.kind_id),
                bounding_box: p.bounding_box,
                gen_depth: p.gen_depth,
                orientation: p.orientation,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            })
            .collect();

        Some(GenerationStub {
            position: (biome_x, 64, biome_z),
            pieces,
        })
    }
}
