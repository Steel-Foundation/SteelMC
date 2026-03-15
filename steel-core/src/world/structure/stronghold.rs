//! Stronghold piece generation for structure starts.
//!
//! Implements vanilla's `StrongholdPieces` recursive BFS to generate piece
//! bounding boxes. Does not place actual blocks.

use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction};

const MAX_DEPTH: i32 = 50;
const MAX_DISTANCE: i32 = 112;
const LOWEST_Y: i32 = 10;

const HORIZONTAL_DIRS: [Direction; 4] = [
    Direction::North, Direction::East, Direction::South, Direction::West,
];

fn random_horizontal(rng: &mut LegacyRandom) -> Direction {
    HORIZONTAL_DIRS[rng.next_i32_bounded(4) as usize]
}

/// Vanilla's `BoundingBox.orientBox`.
fn orient_box(
    fx: i32, fy: i32, fz: i32,
    ox: i32, oy: i32, oz: i32,
    w: i32, h: i32, d: i32,
    dir: Direction,
) -> BoundingBox {
    match dir {
        Direction::North => BoundingBox::new(fx+ox, fy+oy, fz-d+1+oz, fx+w-1+ox, fy+h-1+oy, fz+oz),
        Direction::West => BoundingBox::new(fx-d+1+oz, fy+oy, fz+ox, fx+oz, fy+h-1+oy, fz+w-1+ox),
        Direction::East => BoundingBox::new(fx+oz, fy+oy, fz+ox, fx+d-1+oz, fy+h-1+oy, fz+w-1+ox),
        // South + default
        _ => BoundingBox::new(fx+ox, fy+oy, fz+oz, fx+w-1+ox, fy+h-1+oy, fz+d-1+oz),
    }
}

fn is_ok(bb: &BoundingBox) -> bool { bb.min_y > LOWEST_Y }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PT { Straight, Prison, LeftTurn, RightTurn, RoomCrossing, StraightStairs, StairsDown, FiveCrossing, ChestCorridor, Library, Portal, Filler }

struct PieceWeight { pt: PT, weight: i32, max: i32, count: i32, min_depth: i32 }
impl PieceWeight {
    fn can(&self, depth: i32) -> bool { (self.max == 0 || self.count < self.max) && depth >= self.min_depth }
}

fn weights() -> Vec<PieceWeight> {
    vec![
        PieceWeight { pt: PT::Straight, weight: 40, max: 0, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::Prison, weight: 5, max: 5, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::LeftTurn, weight: 20, max: 0, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::RightTurn, weight: 20, max: 0, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::RoomCrossing, weight: 10, max: 6, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::StraightStairs, weight: 5, max: 5, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::StairsDown, weight: 5, max: 5, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::FiveCrossing, weight: 5, max: 4, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::ChestCorridor, weight: 5, max: 4, count: 0, min_depth: 0 },
        PieceWeight { pt: PT::Library, weight: 10, max: 2, count: 0, min_depth: 5 },
        PieceWeight { pt: PT::Portal, weight: 20, max: 1, count: 0, min_depth: 6 },
    ]
}

struct Piece {
    bb: BoundingBox,
    dir: Direction,
    depth: i32,
    pt: PT,
    // Per-piece constructor state
    left_child: bool,   // Straight
    right_child: bool,  // Straight
    left_low: bool,     // FiveCrossing
    left_high: bool,    // FiveCrossing
    right_low: bool,    // FiveCrossing
    right_high: bool,   // FiveCrossing
    is_tall: bool,      // Library
}

impl Piece {
    fn new(bb: BoundingBox, dir: Direction, depth: i32, pt: PT) -> Self {
        Self { bb, dir, depth, pt, left_child: false, right_child: false,
               left_low: false, left_high: false, right_low: false, right_high: false, is_tall: false }
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

    /// Vanilla's `updatePieceWeight` — returns false if no limited pieces
    /// can still be placed. This STOPS generation even if unlimited pieces remain.
    fn update_weights(&mut self) -> bool {
        let mut has_any = false;
        self.total_weight = 0;
        for w in &self.wts {
            // Only limited pieces (max > 0) that still have room count
            if w.max > 0 && w.count < w.max {
                has_any = true;
            }
            self.total_weight += w.weight;
        }
        has_any
    }
}

fn find_box(pt: PT, s: &State, fx: i32, fy: i32, fz: i32, dir: Direction) -> Option<BoundingBox> {
    let bb = match pt {
        PT::Straight | PT::ChestCorridor => orient_box(fx,fy,fz, -1,-1,0, 5,5,7, dir),
        PT::StairsDown => orient_box(fx,fy,fz, -1,-7,0, 5,11,5, dir),
        PT::StraightStairs => orient_box(fx,fy,fz, -1,-7,0, 5,11,8, dir),
        PT::LeftTurn | PT::RightTurn => orient_box(fx,fy,fz, -1,-1,0, 5,5,5, dir),
        PT::RoomCrossing => orient_box(fx,fy,fz, -4,-1,0, 11,7,11, dir),
        PT::Prison => orient_box(fx,fy,fz, -1,-1,0, 9,5,11, dir),
        PT::FiveCrossing => orient_box(fx,fy,fz, -4,-3,0, 10,9,11, dir),
        PT::Portal => orient_box(fx,fy,fz, -4,-1,0, 11,8,16, dir),
        PT::Library => {
            let tall = orient_box(fx,fy,fz, -4,-1,0, 14,11,15, dir);
            if is_ok(&tall) && !s.collides(&tall) { return Some(tall); }
            orient_box(fx,fy,fz, -4,-1,0, 14,6,15, dir)
        }
        PT::Filler => {
            // Vanilla's FillerCorridor.findPieceBox:
            // 1. Create 5×5×4 box and check for collision
            // 2. If NO collision: return None (filler not needed)
            // 3. If collision at same Y: try shorter (2, 1) and return longest fitting
            let full_box = orient_box(fx,fy,fz, -1,-1,0, 5,5,4, dir);
            // Find colliding piece
            let collision = s.pieces.iter().find(|p| p.bb.intersects(&full_box));
            let Some(collision) = collision else { return None; };
            if collision.bb.min_y != full_box.min_y { return None; }
            let collision_bb = collision.bb;
            for d in (1..=2).rev() {
                let b = orient_box(fx,fy,fz, -1,-1,0, 5,5,d, dir);
                if !collision_bb.intersects(&b) {
                    return Some(orient_box(fx,fy,fz, -1,-1,0, 5,5,d+1, dir));
                }
            }
            return None;
        }
    };
    if is_ok(&bb) && !s.collides(&bb) { Some(bb) } else { None }
}

/// Consume constructor RNG and create piece with stored state.
fn create_piece(pt: PT, bb: BoundingBox, dir: Direction, depth: i32, rng: &mut LegacyRandom) -> Piece {
    let mut p = Piece::new(bb, dir, depth, pt);
    match pt {
        PT::Straight => {
            rng.next_i32_bounded(5); // randomSmallDoor
            p.left_child = rng.next_i32_bounded(2) == 0;
            p.right_child = rng.next_i32_bounded(2) == 0;
        }
        PT::FiveCrossing => {
            rng.next_i32_bounded(5); // randomSmallDoor
            p.left_low = rng.next_i32_bounded(2) == 0;  // nextBoolean
            p.left_high = rng.next_i32_bounded(2) == 0;
            p.right_low = rng.next_i32_bounded(2) == 0;
            p.right_high = rng.next_i32_bounded(3) > 0;  // nextInt(3) > 0
        }
        PT::RoomCrossing => {
            rng.next_i32_bounded(5); // randomSmallDoor
            rng.next_i32_bounded(5); // type (unused for BB)
        }
        PT::Library => {
            rng.next_i32_bounded(5); // randomSmallDoor
            p.is_tall = bb.max_y - bb.min_y + 1 > 6;
        }
        PT::Portal | PT::Filler => {
            // No RNG consumption
        }
        _ => {
            // StairsDown, ChestCorridor, StraightStairs, LeftTurn, RightTurn, Prison
            // All consume randomSmallDoor only
            rng.next_i32_bounded(5);
        }
    }
    p
}

/// Vanilla's `generatePieceFromSmallDoor` — select piece type and create it.
/// Returns None if no piece can be placed (stops generation on this branch).
fn generate_piece(s: &mut State, rng: &mut LegacyRandom, fx: i32, fy: i32, fz: i32, dir: Direction, depth: i32) -> Option<Piece> {
    if !s.update_weights() {
        return None;
    }

    // Try imposed piece first
    if let Some(imp) = s.imposed.take() {
        if let Some(bb) = find_box(imp, s, fx, fy, fz, dir) {
            return Some(create_piece(imp, bb, dir, depth, rng));
        }
    }

    // Weighted random selection (up to 5 attempts)
    // Vanilla uses totalWeight (sum of ALL weights), selects, THEN checks eligibility
    for _ in 0..5 {
        if s.total_weight <= 0 { break; }
        let mut choice = rng.next_i32_bounded(s.total_weight);

        for wi in 0..s.wts.len() {
            choice -= s.wts[wi].weight;
            if choice < 0 {
                // Check if this piece can be placed
                if !s.wts[wi].can(depth) || Some(s.wts[wi].pt) == s.prev_pt {
                    break; // Retry
                }

                if let Some(bb) = find_box(s.wts[wi].pt, s, fx, fy, fz, dir) {
                    let pt = s.wts[wi].pt;
                    let piece = create_piece(pt, bb, dir, depth, rng);
                    s.wts[wi].count += 1;
                    s.prev_pt = Some(pt);
                    // Remove weight if maxed out (vanilla: !piece.isValid())
                    if s.wts[wi].max > 0 && s.wts[wi].count >= s.wts[wi].max {
                        s.wts.remove(wi);
                    }
                    return Some(piece);
                }

                break; // BB didn't fit, retry
            }
        }
    }

    // Fallback: FillerCorridor
    if let Some(bb) = find_box(PT::Filler, s, fx, fy, fz, dir) {
        if bb.min_y > 1 {
            return Some(create_piece(PT::Filler, bb, dir, depth, rng));
        }
    }
    None
}

fn gen_and_add(s: &mut State, rng: &mut LegacyRandom, fx: i32, fy: i32, fz: i32, dir: Direction, depth: i32) {
    if depth > MAX_DEPTH { return; }
    if (fx - s.start_bb.min_x).abs() > MAX_DISTANCE { return; }
    if (fz - s.start_bb.min_z).abs() > MAX_DISTANCE { return; }

    if let Some(piece) = generate_piece(s, rng, fx, fy, fz, dir, depth) {
        let idx = s.pieces.len();
        if piece.pt == PT::Portal { s.has_portal = true; }
        s.pieces.push(piece);
        s.pending.push(idx);
    }
}

fn add_children(s: &mut State, rng: &mut LegacyRandom, idx: usize) {
    let bb = s.pieces[idx].bb;
    let dir = s.pieces[idx].dir;
    let depth = s.pieces[idx].depth;
    let pt = s.pieces[idx].pt;

    match pt {
        PT::StairsDown => {
            // StartPiece (isSource) sets imposedPiece = FiveCrossing
            if depth == 0 { s.imposed = Some(PT::FiveCrossing); }
            fwd(s, rng, bb, dir, depth, 1, 1);
        }
        PT::StraightStairs | PT::ChestCorridor | PT::Prison => {
            fwd(s, rng, bb, dir, depth, 1, 1);
        }
        PT::Straight => {
            let lc = s.pieces[idx].left_child;
            let rc = s.pieces[idx].right_child;
            fwd(s, rng, bb, dir, depth, 1, 1);
            if lc { left(s, rng, bb, dir, depth, 1, 2); }
            if rc { right(s, rng, bb, dir, depth, 1, 2); }
        }
        PT::LeftTurn => { left(s, rng, bb, dir, depth, 1, 1); }
        PT::RightTurn => { right(s, rng, bb, dir, depth, 1, 1); }
        PT::RoomCrossing => {
            fwd(s, rng, bb, dir, depth, 4, 1);
            left(s, rng, bb, dir, depth, 1, 4);
            right(s, rng, bb, dir, depth, 1, 4);
        }
        PT::FiveCrossing => {
            let ll = s.pieces[idx].left_low;
            let lh = s.pieces[idx].left_high;
            let rl = s.pieces[idx].right_low;
            let rh = s.pieces[idx].right_high;
            let (za, zb) = if dir == Direction::West || dir == Direction::North {
                (8 - 3, 8 - 5)
            } else {
                (3, 5)
            };
            fwd(s, rng, bb, dir, depth, 5, 1);
            if ll { left(s, rng, bb, dir, depth, za, 1); }
            if lh { left(s, rng, bb, dir, depth, zb, 7); }
            if rl { right(s, rng, bb, dir, depth, za, 1); }
            if rh { right(s, rng, bb, dir, depth, zb, 7); }
        }
        PT::Library | PT::Filler | PT::Portal => {}
    }
}

/// Vanilla's `generateSmallDoorChildForward(startPiece, accessor, random, xOff, yOff)`.
fn fwd(s: &mut State, rng: &mut LegacyRandom, bb: BoundingBox, dir: Direction, depth: i32, x_off: i32, y_off: i32) {
    match dir {
        Direction::North => gen_and_add(s, rng, bb.min_x+x_off, bb.min_y+y_off, bb.min_z-1, dir, depth+1),
        Direction::South => gen_and_add(s, rng, bb.min_x+x_off, bb.min_y+y_off, bb.max_z+1, dir, depth+1),
        Direction::West => gen_and_add(s, rng, bb.min_x-1, bb.min_y+y_off, bb.min_z+x_off, dir, depth+1),
        Direction::East => gen_and_add(s, rng, bb.max_x+1, bb.min_y+y_off, bb.min_z+x_off, dir, depth+1),
        _ => {}
    }
}

/// Vanilla's `generateSmallDoorChildLeft(startPiece, accessor, random, yOff, zOff)`.
fn left(s: &mut State, rng: &mut LegacyRandom, bb: BoundingBox, dir: Direction, depth: i32, y_off: i32, z_off: i32) {
    match dir {
        Direction::North => gen_and_add(s, rng, bb.min_x-1, bb.min_y+y_off, bb.min_z+z_off, Direction::West, depth+1),
        Direction::South => gen_and_add(s, rng, bb.max_x+1, bb.min_y+y_off, bb.min_z+z_off, Direction::East, depth+1),
        Direction::West => gen_and_add(s, rng, bb.min_x+z_off, bb.min_y+y_off, bb.max_z+1, Direction::South, depth+1),
        Direction::East => gen_and_add(s, rng, bb.min_x+z_off, bb.min_y+y_off, bb.min_z-1, Direction::North, depth+1),
        _ => {}
    }
}

/// Vanilla's `generateSmallDoorChildRight(startPiece, accessor, random, yOff, zOff)`.
fn right(s: &mut State, rng: &mut LegacyRandom, bb: BoundingBox, dir: Direction, depth: i32, y_off: i32, z_off: i32) {
    match dir {
        Direction::North => gen_and_add(s, rng, bb.max_x+1, bb.min_y+y_off, bb.min_z+z_off, Direction::East, depth+1),
        Direction::South => gen_and_add(s, rng, bb.min_x-1, bb.min_y+y_off, bb.min_z+z_off, Direction::West, depth+1),
        Direction::West => gen_and_add(s, rng, bb.min_x+z_off, bb.min_y+y_off, bb.min_z-1, Direction::North, depth+1),
        Direction::East => gen_and_add(s, rng, bb.min_x+z_off, bb.min_y+y_off, bb.max_z+1, Direction::South, depth+1),
        _ => {}
    }
}

/// Generates all stronghold pieces for a chunk.
pub fn generate_pieces(seed: i64, chunk_x: i32, chunk_z: i32) -> Vec<BoundingBox> {
    let west = chunk_x * 16 + 2;
    let north = chunk_z * 16 + 2;

    let mut tries = 0i64;
    loop {
        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(seed.wrapping_add(tries), chunk_x, chunk_z);
        tries += 1;

        // Reset weights
        let start_dir = random_horizontal(&mut rng);
        // StartPiece uses makeBoundingBox (NOT orientBox) — no offsets
        let start_bb = match start_dir {
            Direction::North | Direction::South =>
                BoundingBox::new(west, 64, north, west + 4, 74, north + 4),
            _ => // East/West: swap width and depth
                BoundingBox::new(west, 64, north, west + 4, 74, north + 4),
        };

        let mut s = State {
            pieces: Vec::new(),
            pending: Vec::new(),
            wts: weights(),
            start_bb,
            prev_pt: None,
            has_portal: false,
            imposed: None,
            total_weight: 0,
        };

        // StartPiece (StairsDown with isSource=true)
        // StartPiece constructor does NOT consume RNG (entryDoor = OPENING, not random)
        let start_piece = Piece::new(start_bb, start_dir, 0, PT::StairsDown);
        s.pieces.push(start_piece);

        // StartPiece.addChildren generates one forward child
        add_children(&mut s, &mut rng, 0);

        // BFS: process pending children randomly
        while !s.pending.is_empty() {
            let idx = rng.next_i32_bounded(s.pending.len() as i32) as usize;
            let piece_idx = s.pending.remove(idx);
            add_children(&mut s, &mut rng, piece_idx);
        }

        if !s.pieces.is_empty() && s.has_portal {
            // moveBelowSeaLevel(seaLevel=63, minY=-64, random, offset=10)
            let sea_level = 63;
            let min_y = -64;
            let max_y = sea_level - 10; // 53
            // Compute overall BB
            let mut overall = s.pieces[0].bb;
            for p in &s.pieces[1..] {
                overall = BoundingBox::new(
                    overall.min_x.min(p.bb.min_x), overall.min_y.min(p.bb.min_y), overall.min_z.min(p.bb.min_z),
                    overall.max_x.max(p.bb.max_x), overall.max_y.max(p.bb.max_y), overall.max_z.max(p.bb.max_z),
                );
            }
            let y_span = overall.max_y - overall.min_y + 1;
            let mut y1_pos = y_span + min_y + 1;
            if y1_pos < max_y {
                y1_pos += rng.next_i32_bounded(max_y - y1_pos);
            }
            let dy = y1_pos - overall.max_y;

            return s.pieces.into_iter().map(|p| BoundingBox::new(
                p.bb.min_x, p.bb.min_y + dy, p.bb.min_z,
                p.bb.max_x, p.bb.max_y + dy, p.bb.max_z,
            )).collect();
        }
    }
}
