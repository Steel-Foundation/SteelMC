//! Rotation enum for structure template placement.
//!
//! Vanilla's `Rotation` — horizontal rotations around the Y axis.

use crate::{BoundingBox, Direction};
use crate::random::Random;
use crate::random::legacy_random::LegacyRandom;

/// Horizontal rotation around the Y axis.
///
/// Vanilla's `Rotation` enum. Used for structure template placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// No rotation (0°).
    None,
    /// 90° clockwise.
    Clockwise90,
    /// 180°.
    Clockwise180,
    /// 270° clockwise (= 90° counter-clockwise).
    CounterClockwise90,
}

/// All rotation variants in vanilla's ordinal order.
const ALL_ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::CounterClockwise90,
];

impl Rotation {
    /// Picks a random rotation. Matches `Rotation.getRandom(random)`.
    #[must_use]
    pub fn get_random(rng: &mut LegacyRandom) -> Self {
        ALL_ROTATIONS[rng.next_i32_bounded(4) as usize]
    }

    /// Returns a shuffled copy of all rotations.
    ///
    /// Matches vanilla's `Rotation.getShuffled(random)` which calls
    /// `Util.shuffledCopy(values(), random)`.
    #[must_use]
    pub fn get_shuffled(rng: &mut LegacyRandom) -> [Rotation; 4] {
        let mut rotations = ALL_ROTATIONS;
        // Vanilla's Util.shuffle: reverse Fisher-Yates
        for i in (1..4).rev() {
            let j = rng.next_i32_bounded((i + 1) as i32) as usize;
            rotations.swap(i, j);
        }
        rotations
    }

    /// Rotates a horizontal direction by this rotation.
    ///
    /// Vertical directions (Up/Down) are unchanged.
    #[must_use]
    pub const fn rotate(self, dir: Direction) -> Direction {
        match self {
            Self::None => dir,
            Self::Clockwise90 => dir.rotate_y_clockwise(),
            Self::Clockwise180 => dir.rotate_y_clockwise().rotate_y_clockwise(),
            Self::CounterClockwise90 => dir.rotate_y_counter_clockwise(),
        }
    }

    /// Composes two rotations. `self.then(other)` = apply self first, then other.
    #[must_use]
    pub const fn then(self, other: Self) -> Self {
        let total = (self as u8 + other as u8) % 4;
        ALL_ROTATIONS[total as usize]
    }

    /// Transforms a position by this rotation around a pivot point.
    ///
    /// Matches vanilla's `StructureTemplate.transform(pos, Mirror.NONE, rotation, pivot)`.
    #[must_use]
    pub const fn transform_pos(self, x: i32, y: i32, z: i32, pivot_x: i32, pivot_z: i32) -> (i32, i32, i32) {
        match self {
            Self::None => (x, y, z),
            Self::Clockwise90 => (pivot_x + pivot_z - z, y, pivot_z - pivot_x + x),
            Self::Clockwise180 => (pivot_x + pivot_x - x, y, pivot_z + pivot_z - z),
            Self::CounterClockwise90 => (pivot_x - pivot_z + z, y, pivot_x + pivot_z - x),
        }
    }

    /// Returns the template size after applying this rotation.
    ///
    /// 90° and 270° rotations swap the X and Z dimensions.
    #[must_use]
    pub const fn rotate_size(self, size_x: i32, size_y: i32, size_z: i32) -> (i32, i32, i32) {
        match self {
            Self::Clockwise90 | Self::CounterClockwise90 => (size_z, size_y, size_x),
            Self::None | Self::Clockwise180 => (size_x, size_y, size_z),
        }
    }

    /// Computes the bounding box for a structure template placed at `position` with this rotation.
    ///
    /// Matches vanilla's `StructureTemplate.getBoundingBox(position, rotation, pivot=ZERO, mirror=NONE, size)`.
    /// Jigsaw pool elements always use pivot=ZERO and mirror=NONE.
    #[must_use]
    pub fn get_bounding_box(self, pos_x: i32, pos_y: i32, pos_z: i32, size_x: i32, size_y: i32, size_z: i32) -> BoundingBox {
        let dx = size_x - 1;
        let dy = size_y - 1;
        let dz = size_z - 1;

        // Transform corners with pivot=(0,0,0)
        let (c1x, c1y, c1z) = self.transform_pos(0, 0, 0, 0, 0);
        let (c2x, c2y, c2z) = self.transform_pos(dx, dy, dz, 0, 0);

        // fromCorners takes min/max, then move by position
        BoundingBox::new(
            c1x.min(c2x) + pos_x,
            c1y.min(c2y) + pos_y,
            c1z.min(c2z) + pos_z,
            c1x.max(c2x) + pos_x,
            c1y.max(c2y) + pos_y,
            c1z.max(c2z) + pos_z,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_direction() {
        assert_eq!(Rotation::None.rotate(Direction::North), Direction::North);
        assert_eq!(Rotation::Clockwise90.rotate(Direction::North), Direction::East);
        assert_eq!(Rotation::Clockwise180.rotate(Direction::North), Direction::South);
        assert_eq!(Rotation::CounterClockwise90.rotate(Direction::North), Direction::West);
    }

    #[test]
    fn compose_rotations() {
        assert_eq!(Rotation::Clockwise90.then(Rotation::Clockwise90), Rotation::Clockwise180);
        assert_eq!(Rotation::Clockwise90.then(Rotation::CounterClockwise90), Rotation::None);
        assert_eq!(Rotation::Clockwise180.then(Rotation::Clockwise180), Rotation::None);
    }

    #[test]
    fn vertical_unchanged() {
        assert_eq!(Rotation::Clockwise90.rotate(Direction::Up), Direction::Up);
        assert_eq!(Rotation::Clockwise180.rotate(Direction::Down), Direction::Down);
    }

    #[test]
    fn transform_pos_pivot_zero() {
        // NONE: identity
        assert_eq!(Rotation::None.transform_pos(3, 5, 7, 0, 0), (3, 5, 7));
        // CW_90: (px + pz - z, y, pz - px + x) = (0+0-7, 5, 0-0+3) = (-7, 5, 3)
        assert_eq!(Rotation::Clockwise90.transform_pos(3, 5, 7, 0, 0), (-7, 5, 3));
        // CW_180: (2px - x, y, 2pz - z) = (-3, 5, -7)
        assert_eq!(Rotation::Clockwise180.transform_pos(3, 5, 7, 0, 0), (-3, 5, -7));
        // CCW_90: (px - pz + z, y, px + pz - x) = (7, 5, -3)
        assert_eq!(Rotation::CounterClockwise90.transform_pos(3, 5, 7, 0, 0), (7, 5, -3));
    }

    #[test]
    fn bounding_box_none() {
        // Size (6, 10, 6), position (0,0,0), rotation NONE
        // delta = (5, 9, 5), corners: (0,0,0) and (5,9,5)
        let bb = Rotation::None.get_bounding_box(0, 0, 0, 6, 10, 6);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (0, 0, 0));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (5, 9, 5));
    }

    #[test]
    fn bounding_box_cw90() {
        // Size (6, 10, 8), position (100, 50, 200), rotation CW_90
        // delta = (5, 9, 7)
        // corner1 = transform(0,0,0) = (0,0,0)
        // corner2 = transform(5,9,7) = (0+0-7, 9, 0-0+5) = (-7, 9, 5)
        // min = (-7, 0, 0), max = (0, 9, 5), moved by (100, 50, 200)
        let bb = Rotation::Clockwise90.get_bounding_box(100, 50, 200, 6, 10, 8);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (93, 50, 200));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (100, 59, 205));
    }

    #[test]
    fn bounding_box_cw180() {
        // Size (6, 10, 8), position (0, 0, 0), rotation CW_180
        // delta = (5, 9, 7)
        // corner1 = (0,0,0), corner2 = (-5, 9, -7)
        let bb = Rotation::Clockwise180.get_bounding_box(0, 0, 0, 6, 10, 8);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (-5, 0, -7));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (0, 9, 0));
    }

    #[test]
    fn rotate_size() {
        assert_eq!(Rotation::None.rotate_size(6, 10, 8), (6, 10, 8));
        assert_eq!(Rotation::Clockwise90.rotate_size(6, 10, 8), (8, 10, 6));
        assert_eq!(Rotation::Clockwise180.rotate_size(6, 10, 8), (6, 10, 8));
        assert_eq!(Rotation::CounterClockwise90.rotate_size(6, 10, 8), (8, 10, 6));
    }
}
