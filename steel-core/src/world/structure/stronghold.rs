//! Stronghold piece generation. Vanilla's `StrongholdPieces` recursive BFS;
//! produces bounding boxes only (no blocks).

use steel_registry::structure::StructureData;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction, Identifier};

use crate::world::structure::{
    GenerationStub, Structure, StructureGenerationContext, StructurePiece,
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
const fn orient_box(
    foot: (i32, i32, i32),
    off: (i32, i32, i32),
    size: (i32, i32, i32),
    dir: Direction,
) -> BoundingBox {
    let (fx, fy, fz) = foot;
    let (ox, oy, oz) = off;
    let (w, h, d) = size;
    match dir {
        Direction::North => BoundingBox::new(
            fx + ox,
            fy + oy,
            fz - d + 1 + oz,
            fx + w - 1 + ox,
            fy + h - 1 + oy,
            fz + oz,
        ),
        Direction::West => BoundingBox::new(
            fx - d + 1 + oz,
            fy + oy,
            fz + ox,
            fx + oz,
            fy + h - 1 + oy,
            fz + w - 1 + ox,
        ),
        Direction::East => BoundingBox::new(
            fx + oz,
            fy + oy,
            fz + ox,
            fx + d - 1 + oz,
            fy + h - 1 + oy,
            fz + w - 1 + ox,
        ),
        // South + default
        _ => BoundingBox::new(
            fx + ox,
            fy + oy,
            fz + oz,
            fx + w - 1 + ox,
            fy + h - 1 + oy,
            fz + d - 1 + oz,
        ),
    }
}

const fn is_ok(bb: &BoundingBox) -> bool {
    bb.min_y > LOWEST_Y
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

impl PT {
    const fn vanilla_id(self, depth: i32) -> &'static str {
        match self {
            PT::StairsDown if depth == 0 => "shstart",
            PT::StairsDown => "shsd",
            PT::FiveCrossing => "sh5c",
            PT::Straight => "shs",
            PT::LeftTurn => "shlt",
            PT::RightTurn => "shrt",
            PT::RoomCrossing => "shrc",
            PT::StraightStairs => "shssd",
            PT::ChestCorridor => "shcc",
            PT::Prison => "shph",
            PT::Library => "shli",
            PT::Portal => "shpr",
            PT::Filler => "shfc",
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
    pt: PT,
    /// `Straight`: left/right child flags.
    left_child: bool,
    right_child: bool,
    /// `FiveCrossing`: four door flags.
    left_low: bool,
    left_high: bool,
    right_low: bool,
    right_high: bool,
    /// `Library` height variant.
    is_tall: bool,
}

impl Piece {
    const fn new(bb: BoundingBox, dir: Direction, depth: i32, pt: PT) -> Self {
        Self {
            bb,
            dir,
            depth,
            pt,
            left_child: false,
            right_child: false,
            left_low: false,
            left_high: false,
            right_low: false,
            right_high: false,
            is_tall: false,
        }
    }
}

struct State {
    pieces: Vec<Piece>,
    pending: Vec<usize>,
    wts: Vec<PieceWeight>,
    start_bb: BoundingBox,
    prev_pt: Option<PT>, // last placed piece type (for repeat prevention)
    has_portal: bool,
    imposed: Option<PT>,
    total_weight: i32,
}

impl State {
    fn collides(&self, bb: &BoundingBox) -> bool {
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

fn find_box(pt: PT, s: &State, foot: (i32, i32, i32), dir: Direction) -> Option<BoundingBox> {
    let bb = match pt {
        PT::Straight | PT::ChestCorridor => orient_box(foot, (-1, -1, 0), (5, 5, 7), dir),
        PT::StairsDown => orient_box(foot, (-1, -7, 0), (5, 11, 5), dir),
        PT::StraightStairs => orient_box(foot, (-1, -7, 0), (5, 11, 8), dir),
        PT::LeftTurn | PT::RightTurn => orient_box(foot, (-1, -1, 0), (5, 5, 5), dir),
        PT::RoomCrossing => orient_box(foot, (-4, -1, 0), (11, 7, 11), dir),
        PT::Prison => orient_box(foot, (-1, -1, 0), (9, 5, 11), dir),
        PT::FiveCrossing => orient_box(foot, (-4, -3, 0), (10, 9, 11), dir),
        PT::Portal => orient_box(foot, (-4, -1, 0), (11, 8, 16), dir),
        PT::Library => {
            let tall = orient_box(foot, (-4, -1, 0), (14, 11, 15), dir);
            if is_ok(&tall) && !s.collides(&tall) {
                return Some(tall);
            }
            orient_box(foot, (-4, -1, 0), (14, 6, 15), dir)
        }
        PT::Filler => {
            // Vanilla's FillerCorridor.findPieceBox: 5×5×4 is skipped if no collision;
            // if a same-Y collision exists, try depths (2, 1) and return longest fitting.
            let full_box = orient_box(foot, (-1, -1, 0), (5, 5, 4), dir);
            let collision = s.pieces.iter().find(|p| p.bb.intersects(&full_box))?;
            if collision.bb.min_y != full_box.min_y {
                return None;
            }
            for d in (1..=2).rev() {
                let b = orient_box(foot, (-1, -1, 0), (5, 5, d), dir);
                if !collision.bb.intersects(&b) {
                    return Some(orient_box(foot, (-1, -1, 0), (5, 5, d + 1), dir));
                }
            }
            return None;
        }
    };
    if is_ok(&bb) && !s.collides(&bb) {
        Some(bb)
    } else {
        None
    }
}

/// Consume constructor RNG and create piece with stored state.
fn create_piece(
    pt: PT,
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    rng: &mut LegacyRandom,
) -> Piece {
    let mut p = Piece::new(bb, dir, depth, pt);
    match pt {
        PT::Straight => {
            rng.next_i32_bounded(5);
            // Vanilla uses nextInt(2) == 0, not nextBoolean().
            p.left_child = rng.next_i32_bounded(2) == 0;
            p.right_child = rng.next_i32_bounded(2) == 0;
        }
        PT::FiveCrossing => {
            rng.next_i32_bounded(5);
            p.left_low = rng.next_bool();
            p.left_high = rng.next_bool();
            p.right_low = rng.next_bool();
            p.right_high = rng.next_i32_bounded(3) > 0;
        }
        PT::RoomCrossing => {
            rng.next_i32_bounded(5);
            rng.next_i32_bounded(5); // type, unused for BB
        }
        PT::Library => {
            rng.next_i32_bounded(5);
            p.is_tall = bb.max_y - bb.min_y + 1 > 6;
        }
        PT::Portal | PT::Filler => {}
        // StairsDown, ChestCorridor, StraightStairs, LeftTurn, RightTurn, Prison
        _ => {
            rng.next_i32_bounded(5);
        }
    }
    p
}

/// Vanilla's `generatePieceFromSmallDoor`. Vanilla uses `totalWeight` (sum of all
/// weights), selects, THEN checks eligibility — falls through to subsequent
/// weights in the list on an ineligible pick.
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

    if let Some(imp) = s.imposed.take()
        && let Some(bb) = find_box(imp, s, (fx, fy, fz), dir)
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
                if let Some(bb) = find_box(s.wts[wi].pt, s, (fx, fy, fz), dir) {
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

    // Fallback: FillerCorridor.
    if let Some(bb) = find_box(PT::Filler, s, (fx, fy, fz), dir)
        && bb.min_y > 1
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
        || (fx - s.start_bb.min_x).abs() > MAX_DISTANCE
        || (fz - s.start_bb.min_z).abs() > MAX_DISTANCE
    {
        return;
    }
    if let Some(piece) = generate_piece(s, rng, fx, fy, fz, dir, depth) {
        if piece.pt == PT::Portal {
            s.has_portal = true;
        }
        let idx = s.pieces.len();
        s.pieces.push(piece);
        s.pending.push(idx);
    }
}

fn add_children(s: &mut State, rng: &mut LegacyRandom, idx: usize) {
    let Piece {
        bb, dir, depth, pt, ..
    } = s.pieces[idx];
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
            let (lc, rc) = (s.pieces[idx].left_child, s.pieces[idx].right_child);
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
            let Piece {
                left_low: ll,
                left_high: lh,
                right_low: rl,
                right_high: rh,
                ..
            } = s.pieces[idx];
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

/// Vanilla's `generateSmallDoorChildForward(startPiece, accessor, random, xOff, yOff)`.
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
        Direction::North => (bb.min_x + x_off, bb.min_z - 1),
        Direction::South => (bb.min_x + x_off, bb.max_z + 1),
        Direction::West => (bb.min_x - 1, bb.min_z + x_off),
        Direction::East => (bb.max_x + 1, bb.min_z + x_off),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y + y_off, fz, dir, depth + 1);
}

/// Vanilla's `generateSmallDoorChildLeft`. Vanilla uses identical coords for N/S
/// and for W/E — "left" always means towards minX (or minZ), not relative to facing.
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
        Direction::North | Direction::South => (bb.min_x - 1, bb.min_z + z_off, Direction::West),
        Direction::West | Direction::East => (bb.min_x + z_off, bb.min_z - 1, Direction::North),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y + y_off, fz, d, depth + 1);
}

/// Vanilla's `generateSmallDoorChildRight`. Mirror of [`left`].
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
        Direction::North | Direction::South => (bb.max_x + 1, bb.min_z + z_off, Direction::East),
        Direction::West | Direction::East => (bb.min_x + z_off, bb.max_z + 1, Direction::South),
        _ => return,
    };
    gen_and_add(s, rng, fx, bb.min_y + y_off, fz, d, depth + 1);
}

/// All stronghold pieces for a chunk as
/// `(bounding_box, vanilla_piece_id, orientation, gen_depth)`. Vanilla calls
/// `setOrientation(direction)` on every stronghold piece, and threads
/// `genDepth` through the DFS via each subclass's `createPiece` helper.
#[must_use]
pub fn generate_pieces(
    seed: i64,
    chunk_x: i32,
    chunk_z: i32,
) -> Vec<(BoundingBox, &'static str, Direction, i32)> {
    let west = chunk_x * 16 + 2;
    let north = chunk_z * 16 + 2;

    let mut tries = 0i64;
    loop {
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(seed.wrapping_add(tries), chunk_x, chunk_z);
        tries += 1;

        let start_dir = random_horizontal(&mut rng);
        // StartPiece uses makeBoundingBox (not orientBox). 5×11×5 is square-footprint,
        // so N/S vs E/W produce identical boxes.
        let start_bb = BoundingBox::new(west, 64, north, west + 4, 74, north + 4);

        let mut s = State {
            pieces: vec![Piece::new(start_bb, start_dir, 0, PT::StairsDown)],
            pending: Vec::new(),
            wts: weights(),
            start_bb,
            prev_pt: None,
            has_portal: false,
            imposed: None,
            total_weight: 0,
        };

        // StartPiece.addChildren — no RNG in its constructor (entryDoor = OPENING).
        add_children(&mut s, &mut rng, 0);
        while !s.pending.is_empty() {
            let idx = rng.next_i32_bounded(s.pending.len() as i32) as usize;
            let piece_idx = s.pending.remove(idx);
            add_children(&mut s, &mut rng, piece_idx);
        }

        if s.pieces.is_empty() || !s.has_portal {
            continue;
        }

        // moveBelowSeaLevel(seaLevel=63, minY=-64, offset=10).
        let (min_y, max_y) = (-64, 63 - 10);
        let mut overall = s.pieces[0].bb;
        for p in &s.pieces[1..] {
            overall = BoundingBox::new(
                overall.min_x.min(p.bb.min_x),
                overall.min_y.min(p.bb.min_y),
                overall.min_z.min(p.bb.min_z),
                overall.max_x.max(p.bb.max_x),
                overall.max_y.max(p.bb.max_y),
                overall.max_z.max(p.bb.max_z),
            );
        }
        let mut y1_pos = (overall.max_y - overall.min_y + 1) + min_y + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        let dy = y1_pos - overall.max_y;
        return s
            .pieces
            .into_iter()
            .map(|p| {
                (
                    BoundingBox::new(
                        p.bb.min_x,
                        p.bb.min_y + dy,
                        p.bb.min_z,
                        p.bb.max_x,
                        p.bb.max_y + dy,
                        p.bb.max_z,
                    ),
                    p.pt.vanilla_id(p.depth),
                    p.dir,
                    p.depth,
                )
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
                .map(|(bb, piece_id, dir, depth)| {
                    StructurePiece::non_jigsaw(
                        Identifier::new_static("minecraft", piece_id),
                        bb,
                        depth,
                        Some(dir),
                    )
                })
                .collect(),
        })
    }
}
