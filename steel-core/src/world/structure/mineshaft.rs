//! Mineshaft piece generation for structure starts.
//!
//! Implements vanilla's `MineshaftPieces` logic to generate piece bounding boxes
//! and compute the Y offset for biome checking. Does not place actual blocks.
//!
//! Matches vanilla's DFS recursion: each child's `addChildren` is called
//! immediately after creation, before processing the next sibling.

use steel_utils::density::{ColumnCache, DimensionNoises, NoiseSettings};
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Identifier};

use crate::chunk::aquifer::Aquifer;
use crate::chunk::vanilla_generator::iterate_noise_column_with_aquifer;
use crate::world::structure::placement::StructureSelectionEntry;
use crate::world::structure::{
    GenerationContext, GenerationStub, Structure, StructurePiece,
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
    /// Pieces with their kind + BB, offset to final Y position.
    pub pieces: Vec<(PieceType, BoundingBox)>,
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
        // Vanilla's BoundingBox.getCenter(): min + (max - min + 1) / 2
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
        // moveBelowSeaLevel(seaLevel=63, minY=-64, random, offset=10)
        let max_y = sea_level - 10;
        let y_span = overall.max_y - overall.min_y + 1;
        let mut y1_pos = y_span + min_y + 1;
        if y1_pos < max_y {
            y1_pos += rng.next_i32_bounded(max_y - y1_pos);
        }
        y1_pos - overall.max_y
    };

    // Offset all piece bounding boxes by y_offset, pairing each with its kind.
    let out_pieces = pieces
        .infos
        .iter()
        .map(|info| {
            let bb = BoundingBox::new(
                info.bb.min_x,
                info.bb.min_y + y_offset,
                info.bb.min_z,
                info.bb.max_x,
                info.bb.max_y + y_offset,
                info.bb.max_z,
            );
            (info.kind, bb)
        })
        .collect();

    MineshaftResult {
        biome_check_pos: (middle_x, 50 + y_offset, min_z),
        pieces: out_pieces,
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

/// `Structure` impl — registered under `"minecraft:mineshaft"`. Variant
/// (Normal / Mesa) is resolved from `entry.structure.path`.
pub struct MineshaftStructure;

impl<N: DimensionNoises> Structure<N> for MineshaftStructure {
    fn find_generation_point(
        &self,
        ctx: &mut GenerationContext<'_, '_, N>,
        entry: &StructureSelectionEntry,
        rng: &mut LegacyRandom,
    ) -> Option<GenerationStub> {
        let mtype = if &*entry.structure.path == "mineshaft_mesa" {
            MineshaftType::Mesa
        } else {
            MineshaftType::Normal
        };

        // Mineshaft pieces can span far outside this chunk — the get_height
        // closure builds a fresh aquifer per query at the chunk containing
        // the queried cell (matching vanilla's per-NoiseChunk aquifer).
        let noises = ctx.noises;
        let splitter = ctx.splitter;
        let mut get_height = |x: i32, z: i32| -> i32 {
            let cw = N::Settings::CELL_WIDTH;
            let cell_x = x.div_euclid(cw) * cw;
            let cell_z = z.div_euclid(cw) * cw;
            let aq_chunk_x = (cell_x >> 4) * 16;
            let aq_chunk_z = (cell_z >> 4) * 16;
            let aq_cache = N::ColumnCache::default();
            let mut fresh_aq = Aquifer::<N>::new(
                aq_chunk_x,
                aq_chunk_z,
                N::Settings::MIN_Y,
                N::Settings::HEIGHT,
                splitter,
                noises,
                aq_cache,
            );
            let mut fresh_cache = N::ColumnCache::default();
            fresh_cache.init_grid(aq_chunk_x, aq_chunk_z, noises);
            iterate_noise_column_with_aquifer::<N>(
                &mut fresh_cache,
                noises,
                &mut fresh_aq,
                x,
                z,
                false,
            )
        };

        let result = find_generation_point(
            rng,
            ctx.chunk_x,
            ctx.chunk_z,
            mtype,
            ctx.sea_level,
            N::Settings::MIN_Y,
            &mut get_height,
        );

        let (bx, by, bz) = result.biome_check_pos;
        let biome = ctx.biome_at(bx, by, bz);
        if !entry.allowed_biomes.contains(&biome.key) {
            return None;
        }

        let pieces = result
            .pieces
            .into_iter()
            .map(|(kind, bb)| StructurePiece {
                piece_type: Identifier::new_static("minecraft", kind.piece_id()),
                bounding_box: bb,
                gen_depth: 0,
                orientation: None,
                nbt_data: Vec::new(),
                ground_level_delta: 0,
                junctions: Vec::new(),
            })
            .collect();

        Some(GenerationStub {
            position: result.biome_check_pos,
            pieces,
        })
    }
}
