use glam::IVec3;
use steel_utils::random::Random;
use steel_utils::random::legacy_random::LegacyRandom;
use steel_utils::{BoundingBox, Direction};

const VANILLA_HORIZONTAL_DIRECTIONS: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// Matches vanilla's `Direction.Plane.HORIZONTAL.getRandomDirection`.
pub(crate) fn random_horizontal_direction(rng: &mut LegacyRandom) -> Direction {
    VANILLA_HORIZONTAL_DIRECTIONS[rng.next_i32_bounded(4) as usize]
}

/// Vanilla's `StructurePiece.makeBoundingBox`: north/south keep width/depth,
/// east/west swap them.
pub(crate) const fn make_oriented_piece_bounding_box(
    chunk_min_x: i32,
    y: i32,
    chunk_min_z: i32,
    orientation: Direction,
    width: i32,
    height: i32,
    depth: i32,
) -> BoundingBox {
    let z_axis = matches!(orientation, Direction::North | Direction::South);
    let (box_width, box_depth) = if z_axis {
        (width, depth)
    } else {
        (depth, width)
    };
    BoundingBox::new(
        IVec3::new(chunk_min_x, y, chunk_min_z),
        IVec3::new(
            chunk_min_x + box_width - 1,
            y + height - 1,
            chunk_min_z + box_depth - 1,
        ),
    )
}
