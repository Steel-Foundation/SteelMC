//! Mineshaft piece generation for structure starts.
//!
//! Implements vanilla's `MineshaftPieces` logic to generate piece bounding boxes
//! and compute the Y offset for biome checking. Does not place actual blocks.
//!
//! Matches vanilla's DFS recursion: each child's `addChildren` is called
//! immediately after creation, before processing the next sibling.

use steel_utils::BoundingBox;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;

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

/// Result of mineshaft generation.
pub struct MineshaftResult {
    /// Biome check position `(block_x, block_y, block_z)`.
    pub biome_check_pos: (i32, i32, i32),
    /// Bounding boxes of all generated pieces, offset to final Y position.
    pub piece_bbs: Vec<BoundingBox>,
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
        infos: vec![PieceInfo {
            bb: room_bb,
            kind: PieceType::Room,
            depth: 0,
            dir: None,
        }],
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
        let center_x = i32::midpoint(overall.min_x, overall.max_x);
        let center_z = i32::midpoint(overall.min_z, overall.max_z);
        let surface_height = get_surface_height(center_x, center_z);
        let target = if surface_height <= sea_level {
            sea_level
        } else {
            rng.next_i32_between(sea_level, surface_height)
        };
        let center_y = i32::midpoint(overall.min_y, overall.max_y);
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

    // Offset all piece bounding boxes by y_offset
    let piece_bbs = pieces
        .bbs
        .iter()
        .map(|bb| {
            BoundingBox::new(
                bb.min_x,
                bb.min_y + y_offset,
                bb.min_z,
                bb.max_x,
                bb.max_y + y_offset,
                bb.max_z,
            )
        })
        .collect();

    MineshaftResult {
        biome_check_pos: (middle_x, 50 + y_offset, min_z),
        piece_bbs,
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
        generate_and_add(
            pieces,
            rng,
            bb.min_x + pos,
            fy,
            bb.min_z - 1,
            Dir::North,
            depth,
        );
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
        generate_and_add(
            pieces,
            rng,
            bb.min_x + pos,
            fy,
            bb.max_z + 1,
            Dir::South,
            depth,
        );
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
        generate_and_add(
            pieces,
            rng,
            bb.min_x - 1,
            fy,
            bb.min_z + pos,
            Dir::West,
            depth,
        );
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
        generate_and_add(
            pieces,
            rng,
            bb.max_x + 1,
            fy,
            bb.min_z + pos,
            Dir::East,
            depth,
        );
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

    // createRandomShaftPiece — vanilla uses if/else if/else (no fallthrough!)
    let roll = rng.next_i32_bounded(100);

    if roll >= 80 {
        try_add_crossing(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1);
    } else if roll >= 70 {
        try_add_stairs(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1);
    } else {
        try_add_corridor(pieces, rng, foot_x, foot_y, foot_z, dir, depth + 1);
    }
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
            pieces.infos.push(PieceInfo {
                bb,
                kind: PieceType::Corridor,
                depth: gen_depth,
                dir: Some(dir),
            });
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
    pieces.infos.push(PieceInfo {
        bb,
        kind: PieceType::Crossing,
        depth: gen_depth,
        dir: Some(dir),
    });
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
    pieces.infos.push(PieceInfo {
        bb,
        kind: PieceType::Stairs,
        depth: gen_depth,
        dir: Some(dir),
    });
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
                generate_and_add(
                    pieces,
                    rng,
                    bb.min_x - 1,
                    fy,
                    bb.max_z - 3,
                    Dir::West,
                    depth,
                );
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(
                    pieces,
                    rng,
                    bb.max_x + 1,
                    fy,
                    bb.max_z - 3,
                    Dir::East,
                    depth,
                );
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
                generate_and_add(
                    pieces,
                    rng,
                    bb.max_x - 3,
                    fy,
                    bb.min_z - 1,
                    Dir::North,
                    depth,
                );
            } else {
                let fy = bb.min_y - 1 + rng.next_i32_bounded(3);
                generate_and_add(
                    pieces,
                    rng,
                    bb.max_x - 3,
                    fy,
                    bb.max_z + 1,
                    Dir::South,
                    depth,
                );
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
                        generate_and_add(
                            pieces,
                            rng,
                            bb.min_x - 1,
                            bb.min_y,
                            z,
                            Dir::West,
                            depth + 1,
                        );
                    } else if selection == 1 {
                        generate_and_add(
                            pieces,
                            rng,
                            bb.max_x + 1,
                            bb.min_y,
                            z,
                            Dir::East,
                            depth + 1,
                        );
                    }
                    z += 5;
                }
            }
            Dir::West | Dir::East => {
                let mut x = bb.min_x + 3;
                while x + 3 <= bb.max_x {
                    let selection = rng.next_i32_bounded(5);
                    if selection == 0 {
                        generate_and_add(
                            pieces,
                            rng,
                            x,
                            bb.min_y,
                            bb.min_z - 1,
                            Dir::North,
                            depth + 1,
                        );
                    } else if selection == 1 {
                        generate_and_add(
                            pieces,
                            rng,
                            x,
                            bb.min_y,
                            bb.max_z + 1,
                            Dir::South,
                            depth + 1,
                        );
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
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.min_z - 1,
                Dir::North,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.min_x - 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::West,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.max_x + 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::East,
                depth,
            );
        }
        Dir::South => {
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.max_z + 1,
                Dir::South,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.min_x - 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::West,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.max_x + 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::East,
                depth,
            );
        }
        Dir::West => {
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.min_z - 1,
                Dir::North,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.max_z + 1,
                Dir::South,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.min_x - 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::West,
                depth,
            );
        }
        Dir::East => {
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.min_z - 1,
                Dir::North,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y,
                bb.max_z + 1,
                Dir::South,
                depth,
            );
            generate_and_add(
                pieces,
                rng,
                bb.max_x + 1,
                bb.min_y,
                bb.min_z + 1,
                Dir::East,
                depth,
            );
        }
    }

    if is_two_floored {
        if rng.next_bool() {
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y + 4,
                bb.min_z - 1,
                Dir::North,
                depth,
            );
        }
        if rng.next_bool() {
            generate_and_add(
                pieces,
                rng,
                bb.min_x - 1,
                bb.min_y + 4,
                bb.min_z + 1,
                Dir::West,
                depth,
            );
        }
        if rng.next_bool() {
            generate_and_add(
                pieces,
                rng,
                bb.max_x + 1,
                bb.min_y + 4,
                bb.min_z + 1,
                Dir::East,
                depth,
            );
        }
        if rng.next_bool() {
            generate_and_add(
                pieces,
                rng,
                bb.min_x + 1,
                bb.min_y + 4,
                bb.max_z + 1,
                Dir::South,
                depth,
            );
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
        Dir::North => generate_and_add(
            pieces,
            rng,
            bb.min_x,
            bb.min_y,
            bb.min_z - 1,
            Dir::North,
            depth,
        ),
        Dir::South => generate_and_add(
            pieces,
            rng,
            bb.min_x,
            bb.min_y,
            bb.max_z + 1,
            Dir::South,
            depth,
        ),
        Dir::West => generate_and_add(
            pieces,
            rng,
            bb.min_x - 1,
            bb.min_y,
            bb.min_z,
            Dir::West,
            depth,
        ),
        Dir::East => generate_and_add(
            pieces,
            rng,
            bb.max_x + 1,
            bb.min_y,
            bb.min_z,
            Dir::East,
            depth,
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies mineshaft piece generation matches vanilla for seed 13579, chunk (0,0).
    /// Vanilla values from the extractor mixin trace.
    #[test]
    fn mineshaft_matches_vanilla_seed_13579_chunk_0_0() {
        let seed: i64 = 13579;

        let mut rng = LegacyRandom::from_seed(0);
        rng.set_large_feature_seed(seed, 0, 0);
        rng.next_f64(); // consumed by findGenerationPoint

        let room_bb = create_room_bb(&mut rng, 2, 2);
        assert_eq!(room_bb, BoundingBox::new(2, 50, 2, 13, 56, 9));

        let mut pieces = Pieces {
            bbs: vec![room_bb],
            infos: vec![PieceInfo {
                bb: room_bb,
                kind: PieceType::Room,
                depth: 0,
                dir: None,
            }],
            start_bb: room_bb,
        };
        room_add_children(&mut pieces, &mut rng, room_bb);

        // Vanilla produces exactly 92 pieces
        assert_eq!(pieces.bbs.len(), 92);

        // Vanilla's overall bounding box
        let mut overall = pieces.bbs[0];
        for bb in &pieces.bbs[1..] {
            overall = union_bb(overall, *bb);
        }
        assert_eq!(overall, BoundingBox::new(-45, 42, -74, 60, 59, 41));

        // moveBelowSeaLevel produces y_offset = -70 → biome check at Y = -20
        let max_y = 63 - 10; // sea_level - 10
        let y_span = overall.max_y - overall.min_y + 1;
        let mut y1_pos = y_span + (-64) + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        let y_offset = y1_pos - overall.max_y;
        assert_eq!(y_offset, -70);
        assert_eq!(50 + y_offset, -20); // biome check Y
    }
}
