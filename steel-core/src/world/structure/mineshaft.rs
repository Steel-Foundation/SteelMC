//! Mineshaft piece generation for structure starts.
//!
//! Implements vanilla's `MineshaftPieces` logic to generate piece bounding boxes
//! and compute the Y offset for biome checking. Does not place actual blocks.
//!
//! Matches vanilla's DFS recursion: each child's `addChildren` is called
//! immediately after creation, before processing the next sibling.

use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::BoundingBox;

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

#[derive(Debug, Clone, Copy)]
enum PieceType {
    Room,
    Corridor,
    Crossing,
    Stairs,
}

struct PieceInfo {
    bb: BoundingBox,
    kind: PieceType,
    depth: i32,
    dir: Option<Dir>,
}

struct Pieces {
    bbs: Vec<BoundingBox>,
    infos: Vec<PieceInfo>,
    start_bb: BoundingBox,
}

impl Pieces {
    fn has_collision(&self, bb: &BoundingBox) -> bool {
        self.bbs.iter().any(|b| b.intersects(bb))
    }
}

/// Generates mineshaft pieces and returns the biome check position.
///
/// Returns `(block_x, block_y, block_z)` — the position where vanilla checks
/// the biome for this mineshaft.
pub fn find_generation_point(
    rng: &mut LegacyRandom,
    chunk_x: i32,
    chunk_z: i32,
    mtype: MineshaftType,
    sea_level: i32,
    min_y: i32,
    get_surface_height: &mut dyn FnMut(i32, i32) -> i32,
) -> (i32, i32, i32) {
    // Vanilla: context.random().nextDouble() — consumed but unused
    rng.next_f64();

    let middle_x = chunk_x * 16 + 8;
    let min_z = chunk_z * 16;

    // Create the starting room at chunkPos.getBlockX(2), chunkPos.getBlockZ(2)
    let room_x = chunk_x * 16 + 2;
    let room_z = chunk_z * 16 + 2;
    let room_bb = create_room_bb(rng, room_x, room_z);

    let mut pieces = Pieces {
        bbs: vec![room_bb],
        infos: vec![PieceInfo { bb: room_bb, kind: PieceType::Room, depth: 0, dir: None }],
        start_bb: room_bb,
        
    };

    // Room's addChildren — DFS
    room_add_children(&mut pieces, rng, room_bb);

    // Compute overall bounding box
    let mut overall = pieces.bbs[0];
    for bb in &pieces.bbs[1..] {
        overall = union_bb(overall, *bb);
    }

    // Apply vertical adjustment
    let y_offset = if mtype == MineshaftType::Mesa {
        let center_x = (overall.min_x + overall.max_x) / 2;
        let center_z = (overall.min_z + overall.max_z) / 2;
        let surface_height = get_surface_height(center_x, center_z);
        let target = if surface_height <= sea_level {
            sea_level
        } else {
            rng.next_i32_between(sea_level, surface_height)
        };
        let center_y = (overall.min_y + overall.max_y) / 2;
        target - center_y
    } else {
        // moveBelowSeaLevel(seaLevel=63, minY=-64, random, offset=10)
        let max_y = sea_level - 10;
        let y_span = overall.max_y - overall.min_y + 1;
        let mut y1_pos = y_span + min_y + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        y1_pos - overall.max_y
    };

    (middle_x, 50 + y_offset, min_z)
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

// Matches vanilla MineShaftRoom.addChildren exactly.
// When `checkpoints` is Some, stores nextInt() after each wall's DFS completes.
fn room_add_children(pieces: &mut Pieces, rng: &mut LegacyRandom, bb: BoundingBox) {
    let depth = 0; // room is always depth 0
    let x_span = bb.max_x - bb.min_x + 1;
    let z_span = bb.max_z - bb.min_z + 1;
    let mut height_space = (bb.max_y - bb.min_y + 1) - 3 - 1;
    if height_space <= 0 {
        height_space = 1;
    }

    // North wall (iterate over X span)
    let mut pos = 0;
    while pos < x_span {
        pos += rng.next_i32_bounded(x_span);
        if pos + 3 > x_span {
            break;
        }
        let fy = bb.min_y + rng.next_i32_bounded(height_space) + 1;
        generate_and_add(pieces, rng, bb.min_x + pos, fy, bb.min_z - 1, Dir::North, depth);
        pos += 4;
    }

    // South wall (iterate over X span)
    pos = 0;
    while pos < x_span {
        pos += rng.next_i32_bounded(x_span);
        if pos + 3 > x_span {
            break;
        }
        let fy = bb.min_y + rng.next_i32_bounded(height_space) + 1;
        generate_and_add(pieces, rng, bb.min_x + pos, fy, bb.max_z + 1, Dir::South, depth);
        pos += 4;
    }

    // West wall (iterate over Z span)
    pos = 0;
    while pos < z_span {
        pos += rng.next_i32_bounded(z_span);
        if pos + 3 > z_span {
            break;
        }
        let fy = bb.min_y + rng.next_i32_bounded(height_space) + 1;
        generate_and_add(pieces, rng, bb.min_x - 1, fy, bb.min_z + pos, Dir::West, depth);
        pos += 4;
    }

    // East wall (iterate over Z span)
    pos = 0;
    while pos < z_span {
        pos += rng.next_i32_bounded(z_span);
        if pos + 3 > z_span {
            break;
        }
        let fy = bb.min_y + rng.next_i32_bounded(height_space) + 1;
        generate_and_add(pieces, rng, bb.max_x + 1, fy, bb.min_z + pos, Dir::East, depth);
        pos += 4;
    }
}

/// Vanilla's `generateAndAddPiece` — creates a piece, adds it, and
/// immediately calls addChildren (DFS recursion).
fn generate_and_add(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    depth: i32,
) {
    if depth > MAX_DEPTH {
        return;
    }
    if (foot_x - pieces.start_bb.min_x).abs() > MAX_DISTANCE {
        return;
    }
    if (foot_z - pieces.start_bb.min_z).abs() > MAX_DISTANCE {
        return;
    }

    // createRandomShaftPiece
    let roll = rng.next_i32_bounded(100);

    if roll >= 80 {
        if try_add_crossing(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1) {
            return;
        }
    }

    if roll >= 70 {
        if try_add_stairs(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1) {
            return;
        }
    }

    try_add_corridor(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1);
}

fn try_add_corridor(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> bool {
    // Vanilla tries decreasing lengths until one fits
    let mut corridor_length = rng.next_i32_bounded(3) + 2;
    while corridor_length > 0 {
        let block_length = corridor_length * 5;
        let mut bb = match dir {
            Dir::North => BoundingBox::new(0, 0, -(block_length - 1), 2, 2, 0),
            Dir::South => BoundingBox::new(0, 0, 0, 2, 2, block_length - 1),
            Dir::West => BoundingBox::new(-(block_length - 1), 0, 0, 0, 2, 2),
            Dir::East => BoundingBox::new(0, 0, 0, block_length - 1, 2, 2),
        };
        bb = move_bb(bb, foot_x, foot_y, foot_z);

        if !pieces.has_collision(&bb) {
            pieces.bbs.push(bb);
            pieces.infos.push(PieceInfo { bb, kind: PieceType::Corridor, depth: gen_depth, dir: Some(dir) });
            // MineShaftCorridor constructor consumes random state:
            let has_rails = rng.next_i32_bounded(3) == 0;
            if !has_rails {
                rng.next_i32_bounded(23); // spiderCorridor check
            }
            corridor_add_children(pieces, rng, bb, dir, gen_depth);
            return true;
        }
        corridor_length -= 1;
    }
    false
}

fn try_add_crossing(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> bool {
    let is_two_floored = rng.next_i32_bounded(4) == 0;
    let y1 = if is_two_floored { 6 } else { 2 };

    let mut bb = match dir {
        Dir::North => BoundingBox::new(-1, 0, -4, 3, y1, 0),
        Dir::South => BoundingBox::new(-1, 0, 0, 3, y1, 4),
        Dir::West => BoundingBox::new(-4, 0, -1, 0, y1, 3),
        Dir::East => BoundingBox::new(0, 0, -1, 4, y1, 3),
    };
    bb = move_bb(bb, foot_x, foot_y, foot_z);

    if pieces.has_collision(&bb) {
        return false;
    }

    pieces.bbs.push(bb);
    pieces.infos.push(PieceInfo { bb, kind: PieceType::Crossing, depth: gen_depth, dir: Some(dir) });
    crossing_add_children(pieces, rng, bb, dir, gen_depth, is_two_floored);
    true
}

fn try_add_stairs(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    foot_x: i32,
    foot_y: i32,
    foot_z: i32,
    dir: Dir,
    gen_depth: i32,
) -> bool {
    let mut bb = match dir {
        Dir::North => BoundingBox::new(0, -5, -8, 2, 2, 0),
        Dir::South => BoundingBox::new(0, -5, 0, 2, 2, 8),
        Dir::West => BoundingBox::new(-8, -5, 0, 0, 2, 2),
        Dir::East => BoundingBox::new(0, -5, 0, 8, 2, 2),
    };
    bb = move_bb(bb, foot_x, foot_y, foot_z);

    if pieces.has_collision(&bb) {
        return false;
    }

    pieces.bbs.push(bb);
    pieces.infos.push(PieceInfo { bb, kind: PieceType::Stairs, depth: gen_depth, dir: Some(dir) });
    stairs_add_children(pieces, rng, bb, dir, gen_depth);
    true
}

// Matches vanilla MineShaftCorridor.addChildren exactly.
fn corridor_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
) {
    let end_selection = rng.next_i32_bounded(4);

    match dir {
        Dir::North => {
            if end_selection <= 1 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x, fy, bb.min_z - 1, Dir::North, depth);
            } else if end_selection == 2 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x - 1, fy, bb.min_z, Dir::West, depth);
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.max_x + 1, fy, bb.min_z, Dir::East, depth);
            }
        }
        Dir::South => {
            if end_selection <= 1 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x, fy, bb.max_z + 1, Dir::South, depth);
            } else if end_selection == 2 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x - 1, fy, bb.max_z - 3, Dir::West, depth);
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.max_x + 1, fy, bb.max_z - 3, Dir::East, depth);
            }
        }
        Dir::West => {
            if end_selection <= 1 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x - 1, fy, bb.min_z, Dir::West, depth);
            } else if end_selection == 2 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x, fy, bb.min_z - 1, Dir::North, depth);
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.min_x, fy, bb.max_z + 1, Dir::South, depth);
            }
        }
        Dir::East => {
            if end_selection <= 1 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.max_x + 1, fy, bb.min_z, Dir::East, depth);
            } else if end_selection == 2 {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.max_x - 3, fy, bb.min_z - 1, Dir::North, depth);
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(pieces, rng, bb.max_x - 3, fy, bb.max_z + 1, Dir::South, depth);
            }
        }
    }

    // Perpendicular branches along corridor
    if depth < MAX_DEPTH {
        match dir {
            Dir::North | Dir::South => {
                let mut z = bb.min_z + 3;
                while z + 3 <= bb.max_z {
                    let selection = rng.next_i32_bounded(5);
                    if selection == 0 {
                        generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y, z, Dir::West, depth + 1);
                    } else if selection == 1 {
                        generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y, z, Dir::East, depth + 1);
                    }
                    z += 5;
                }
            }
            Dir::West | Dir::East => {
                let mut x = bb.min_x + 3;
                while x + 3 <= bb.max_x {
                    let selection = rng.next_i32_bounded(5);
                    if selection == 0 {
                        generate_and_add(pieces, rng, x, bb.min_y, bb.min_z - 1, Dir::North, depth + 1);
                    } else if selection == 1 {
                        generate_and_add(pieces, rng, x, bb.min_y, bb.max_z + 1, Dir::South, depth + 1);
                    }
                    x += 5;
                }
            }
        }
    }
}

// Matches vanilla MineShaftCrossing.addChildren exactly.
fn crossing_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
    is_two_floored: bool,
) {
    match dir {
        Dir::North => {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.min_z - 1, Dir::North, depth);
            generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y, bb.min_z + 1, Dir::West, depth);
            generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y, bb.min_z + 1, Dir::East, depth);
        }
        Dir::South => {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.max_z + 1, Dir::South, depth);
            generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y, bb.min_z + 1, Dir::West, depth);
            generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y, bb.min_z + 1, Dir::East, depth);
        }
        Dir::West => {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.min_z - 1, Dir::North, depth);
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.max_z + 1, Dir::South, depth);
            generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y, bb.min_z + 1, Dir::West, depth);
        }
        Dir::East => {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.min_z - 1, Dir::North, depth);
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y, bb.max_z + 1, Dir::South, depth);
            generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y, bb.min_z + 1, Dir::East, depth);
        }
    }

    if is_two_floored {
        if rng.next_bool() {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y + 4, bb.min_z - 1, Dir::North, depth);
        }
        if rng.next_bool() {
            generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y + 4, bb.min_z + 1, Dir::West, depth);
        }
        if rng.next_bool() {
            generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y + 4, bb.min_z + 1, Dir::East, depth);
        }
        if rng.next_bool() {
            generate_and_add(pieces, rng, bb.min_x + 1, bb.min_y + 4, bb.max_z + 1, Dir::South, depth);
        }
    }
}

// Matches vanilla MineShaftStairs.addChildren exactly.
fn stairs_add_children(
    pieces: &mut Pieces,
    rng: &mut LegacyRandom,
    bb: BoundingBox,
    dir: Dir,
    depth: i32,
) {
    match dir {
        Dir::North => generate_and_add(pieces, rng, bb.min_x, bb.min_y, bb.min_z - 1, Dir::North, depth),
        Dir::South => generate_and_add(pieces, rng, bb.min_x, bb.min_y, bb.max_z + 1, Dir::South, depth),
        Dir::West => generate_and_add(pieces, rng, bb.min_x - 1, bb.min_y, bb.min_z, Dir::West, depth),
        Dir::East => generate_and_add(pieces, rng, bb.max_x + 1, bb.min_y, bb.min_z, Dir::East, depth),
    }
}

// --- Helpers ---

fn move_bb(bb: BoundingBox, dx: i32, dy: i32, dz: i32) -> BoundingBox {
    BoundingBox::new(
        bb.min_x + dx,
        bb.min_y + dy,
        bb.min_z + dz,
        bb.max_x + dx,
        bb.max_y + dy,
        bb.max_z + dz,
    )
}

/// Simulates the random consumption of the room wall loop without DFS.
fn simulate_room_wall_random(rng: &mut LegacyRandom, bb: BoundingBox) {
    let x_span = bb.max_x - bb.min_x + 1;
    let z_span = bb.max_z - bb.min_z + 1;
    let mut height_space = (bb.max_y - bb.min_y + 1) - 3 - 1;
    if height_space <= 0 {
        height_space = 1;
    }

    // North wall
    let mut pos = 0;
    while pos < x_span {
        pos += rng.next_i32_bounded(x_span);
        if pos + 3 > x_span { break; }
        rng.next_i32_bounded(height_space); // y offset
        // Would call generate_and_add here — consumes nextInt(100) + piece creation randoms
        // But we SKIP it to isolate the wall loop
        pos += 4;
    }

    // South wall
    pos = 0;
    while pos < x_span {
        pos += rng.next_i32_bounded(x_span);
        if pos + 3 > x_span { break; }
        rng.next_i32_bounded(height_space);
        pos += 4;
    }

    // West wall
    pos = 0;
    while pos < z_span {
        pos += rng.next_i32_bounded(z_span);
        if pos + 3 > z_span { break; }
        rng.next_i32_bounded(height_space);
        pos += 4;
    }

    // East wall
    pos = 0;
    while pos < z_span {
        pos += rng.next_i32_bounded(z_span);
        if pos + 3 > z_span { break; }
        rng.next_i32_bounded(height_space);
        pos += 4;
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_mineshaft_chunk_0_0() {
        // Seed 13579, chunk (0,0) — trace key values for vanilla comparison
        let seed: i64 = 13579;
        let chunk_x = 0;
        let chunk_z = 0;

        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(seed, chunk_x, chunk_z);

        // Trace: first few random values after seeding
        let mut trace_rng = LegacyRandom::from_seed(0);
        trace_rng.set_large_feature_seed(seed, chunk_x, chunk_z);
        let r0 = trace_rng.next_f64(); // nextDouble consumed in findGenerationPoint
        let r1 = trace_rng.next_i32_bounded(6); // room width random
        let r2 = trace_rng.next_i32_bounded(6); // room height random
        let r3 = trace_rng.next_i32_bounded(6); // room depth random
        eprintln!("Seed {seed}, chunk ({chunk_x},{chunk_z}):");
        eprintln!("  nextDouble = {r0}");
        eprintln!("  room rands: width_r={r1}, height_r={r2}, depth_r={r3}");

        let room_x = chunk_x * 16 + 2;
        let room_z = chunk_z * 16 + 2;
        let room_bb = BoundingBox::new(
            room_x, 50, room_z,
            room_x + 7 + r1, 54 + r2, room_z + 7 + r3,
        );
        eprintln!("  room_bb: ({},{},{}) -> ({},{},{})",
            room_bb.min_x, room_bb.min_y, room_bb.min_z,
            room_bb.max_x, room_bb.max_y, room_bb.max_z);

        // Now run the full generation
        let (bx, by, bz) = find_generation_point(
            &mut rng, chunk_x, chunk_z,
            MineshaftType::Normal, 63, -64,
            &mut |_, _| 70, // dummy surface height
        );
        eprintln!("  biome_check_pos: ({bx}, {by}, {bz})");
        eprintln!("  piece_count after generation with fresh rng:");

        // Run again to count pieces
        let mut rng2 = LegacyRandom::from_seed(0);
        rng2.set_large_feature_seed(seed, chunk_x, chunk_z);
        rng2.next_f64();
        let room_bb2 = create_room_bb(&mut rng2, room_x, room_z);
        let mut pieces = Pieces {
            bbs: vec![room_bb2],
            infos: vec![PieceInfo { bb: room_bb2, kind: PieceType::Room, depth: 0, dir: None }],
            start_bb: room_bb2, 
        };
        room_add_children(&mut pieces, &mut rng2, room_bb2);
        let mut overall = pieces.bbs[0];
        for bb in &pieces.bbs[1..] {
            overall = union_bb(overall, *bb);
        }
        let random_check = rng2.next_i32();
        eprintln!("  random_check_after_pieces: {random_check}");

        // Check random state at intermediate points
        // After room wall loop (no DFS)
        let mut rng3 = LegacyRandom::from_seed(0);
        rng3.set_large_feature_seed(seed, chunk_x, chunk_z);
        rng3.next_f64();
        let room_bb3 = create_room_bb(&mut rng3, room_x, room_z);
        simulate_room_wall_random(&mut rng3, room_bb3);
        eprintln!("  random_check_after_room_walls: {}", rng3.next_i32());

        // Check how many children each wall generates (with DFS)
        // by counting pieces added during each wall
        let mut rng4 = LegacyRandom::from_seed(0);
        rng4.set_large_feature_seed(seed, chunk_x, chunk_z);
        rng4.next_f64();
        let room_bb4 = create_room_bb(&mut rng4, room_x, room_z);
        let mut p4 = Pieces {
            bbs: vec![room_bb4],
            infos: vec![PieceInfo { bb: room_bb4, kind: PieceType::Room, depth: 0, dir: None }],
            start_bb: room_bb4, 
        };
        let x_span = room_bb4.max_x - room_bb4.min_x + 1;
        let z_span = room_bb4.max_z - room_bb4.min_z + 1;
        let mut hs = (room_bb4.max_y - room_bb4.min_y + 1) - 3 - 1;
        if hs <= 0 { hs = 1; }

        // North wall with DFS — checkpoint after EACH child
        let before = p4.bbs.len();
        let mut pos = 0;
        let mut north_child_idx = 0;
        while pos < x_span {
            pos += rng4.next_i32_bounded(x_span);
            if pos + 3 > x_span { break; }
            let fy = room_bb4.min_y + rng4.next_i32_bounded(hs) + 1;
            let before_child = p4.bbs.len();
            generate_and_add(&mut p4, &mut rng4, room_bb4.min_x + pos, fy, room_bb4.min_z - 1, Dir::North, 0);
            let child_pieces = p4.bbs.len() - before_child;
            north_child_idx += 1;
            pos += 4;
        }
        eprintln!("  north_wall_total: {} pieces", p4.bbs.len() - before);

        // South wall with DFS
        let before = p4.bbs.len();
        pos = 0;
        while pos < x_span {
            pos += rng4.next_i32_bounded(x_span);
            if pos + 3 > x_span { break; }
            let fy = room_bb4.min_y + rng4.next_i32_bounded(hs) + 1;
            generate_and_add(&mut p4, &mut rng4, room_bb4.min_x + pos, fy, room_bb4.max_z + 1, Dir::South, 0);
            pos += 4;
        }
        eprintln!("  south_wall: {} pieces, random_check={}", p4.bbs.len() - before, rng4.next_i32());

        // West wall with DFS
        let before = p4.bbs.len();
        pos = 0;
        while pos < z_span {
            pos += rng4.next_i32_bounded(z_span);
            if pos + 3 > z_span { break; }
            let fy = room_bb4.min_y + rng4.next_i32_bounded(hs) + 1;
            generate_and_add(&mut p4, &mut rng4, room_bb4.min_x - 1, fy, room_bb4.min_z + pos, Dir::West, 0);
            pos += 4;
        }
        eprintln!("  west_wall: {} pieces, random_check={}", p4.bbs.len() - before, rng4.next_i32());

        // East wall with DFS
        let before = p4.bbs.len();
        pos = 0;
        while pos < z_span {
            pos += rng4.next_i32_bounded(z_span);
            if pos + 3 > z_span { break; }
            let fy = room_bb4.min_y + rng4.next_i32_bounded(hs) + 1;
            generate_and_add(&mut p4, &mut rng4, room_bb4.max_x + 1, fy, room_bb4.min_z + pos, Dir::East, 0);
            pos += 4;
        }
        eprintln!("  east_wall: {} pieces, random_check={}", p4.bbs.len() - before, rng4.next_i32());
        eprintln!("  piece_count: {}", pieces.bbs.len());
        eprintln!("  overall_bb: ({},{},{}) -> ({},{},{})",
            overall.min_x, overall.min_y, overall.min_z,
            overall.max_x, overall.max_y, overall.max_z);

        // Dump first 20 pieces for comparison with vanilla
        let dir_name = |d: Option<Dir>| match d {
            None => "-",
            Some(Dir::North) => "N",
            Some(Dir::South) => "S",
            Some(Dir::West) => "W",
            Some(Dir::East) => "E",
        };
        let type_name = |t: PieceType| match t {
            PieceType::Room => "room",
            PieceType::Corridor => "corridor",
            PieceType::Crossing => "crossing",
            PieceType::Stairs => "stairs",
        };
        eprintln!("  First 20 pieces:");
        for (i, info) in pieces.infos.iter().take(20).enumerate() {
            eprintln!("    [{}] {} d={} dir={} bb=({},{},{})→({},{},{})",
                i, type_name(info.kind), info.depth, dir_name(info.dir),
                info.bb.min_x, info.bb.min_y, info.bb.min_z,
                info.bb.max_x, info.bb.max_y, info.bb.max_z);
        }
    }
}
