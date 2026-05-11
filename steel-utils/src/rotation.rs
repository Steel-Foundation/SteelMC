//! Vanilla's `Rotation` — horizontal rotations around the Y axis.

use crate::random::Random;
use crate::random::legacy_random::LegacyRandom;
use crate::{BoundingBox, Direction};

/// Horizontal rotation around the Y axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// 0°.
    None,
    /// 90° clockwise.
    Clockwise90,
    /// 180°.
    Clockwise180,
    /// 270° clockwise (= 90° counter-clockwise).
    CounterClockwise90,
}

const ALL_ROTATIONS: [Rotation; 4] = [
    Rotation::None,
    Rotation::Clockwise90,
    Rotation::Clockwise180,
    Rotation::CounterClockwise90,
];

impl Rotation {
    /// Matches vanilla's `Rotation.getRandom(random)`.
    #[must_use]
    pub fn get_random(rng: &mut LegacyRandom) -> Self {
        ALL_ROTATIONS[rng.next_i32_bounded(4) as usize]
    }

    /// Matches vanilla's `Util.shuffledCopy(values(), random)` (reverse Fisher-Yates).
    #[must_use]
    pub fn get_shuffled(rng: &mut LegacyRandom) -> [Rotation; 4] {
        let mut rotations = ALL_ROTATIONS;
        for i in (1..4).rev() {
            let j = rng.next_i32_bounded((i + 1) as i32) as usize;
            rotations.swap(i, j);
        }
        rotations
    }

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

    /// `self.then(other)` = apply self first, then other.
    #[must_use]
    pub const fn then(self, other: Self) -> Self {
        ALL_ROTATIONS[((self as u8 + other as u8) % 4) as usize]
    }

    /// Matches vanilla's `StructureTemplate.transform(pos, Mirror.NONE, rotation, pivot)`.
    #[must_use]
    pub const fn transform_pos(
        self,
        x: i32,
        y: i32,
        z: i32,
        pivot_x: i32,
        pivot_z: i32,
    ) -> (i32, i32, i32) {
        match self {
            Self::None => (x, y, z),
            Self::Clockwise90 => (pivot_x + pivot_z - z, y, pivot_z - pivot_x + x),
            Self::Clockwise180 => (pivot_x + pivot_x - x, y, pivot_z + pivot_z - z),
            Self::CounterClockwise90 => (pivot_x - pivot_z + z, y, pivot_x + pivot_z - x),
        }
    }

    /// 90°/270° swap the X and Z dimensions.
    #[must_use]
    pub const fn rotate_size(self, size_x: i32, size_y: i32, size_z: i32) -> (i32, i32, i32) {
        match self {
            Self::Clockwise90 | Self::CounterClockwise90 => (size_z, size_y, size_x),
            Self::None | Self::Clockwise180 => (size_x, size_y, size_z),
        }
    }

    /// Matches vanilla's `StructureTemplate.transform(pos, Mirror.FRONT_BACK, rotation, pivot)`.
    #[must_use]
    pub const fn transform_pos_mirrored(
        self,
        x: i32,
        y: i32,
        z: i32,
        pivot_x: i32,
        pivot_z: i32,
        mirror_front_back: bool,
    ) -> (i32, i32, i32) {
        let mx = if mirror_front_back { -x } else { x };
        self.transform_pos(mx, y, z, pivot_x, pivot_z)
    }

    /// Matches vanilla's `StructureTemplate.getBoundingBox(position, rotation, pivot, mirror, size)`.
    #[must_use]
    pub const fn get_bounding_box_full(
        self,
        pos: (i32, i32, i32),
        size: (i32, i32, i32),
        pivot_x: i32,
        pivot_z: i32,
        mirror_front_back: bool,
    ) -> BoundingBox {
        let (c1x, c1y, c1z) =
            self.transform_pos_mirrored(0, 0, 0, pivot_x, pivot_z, mirror_front_back);
        let (c2x, c2y, c2z) = self.transform_pos_mirrored(
            size.0 - 1,
            size.1 - 1,
            size.2 - 1,
            pivot_x,
            pivot_z,
            mirror_front_back,
        );
        BoundingBox::new(
            c1x.min(c2x) + pos.0,
            c1y.min(c2y) + pos.1,
            c1z.min(c2z) + pos.2,
            c1x.max(c2x) + pos.0,
            c1y.max(c2y) + pos.1,
            c1z.max(c2z) + pos.2,
        )
    }

    /// [`get_bounding_box_full`] with `mirror=NONE`.
    #[must_use]
    pub const fn get_bounding_box_with_pivot(
        self,
        pos: (i32, i32, i32),
        size: (i32, i32, i32),
        pivot_x: i32,
        pivot_z: i32,
    ) -> BoundingBox {
        self.get_bounding_box_full(pos, size, pivot_x, pivot_z, false)
    }

    /// [`get_bounding_box_full`] with `pivot=ZERO` and `mirror=NONE`. Used by jigsaw pool elements.
    #[must_use]
    pub const fn get_bounding_box(
        self,
        pos_x: i32,
        pos_y: i32,
        pos_z: i32,
        size_x: i32,
        size_y: i32,
        size_z: i32,
    ) -> BoundingBox {
        self.get_bounding_box_full((pos_x, pos_y, pos_z), (size_x, size_y, size_z), 0, 0, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_direction() {
        assert_eq!(Rotation::None.rotate(Direction::North), Direction::North);
        assert_eq!(
            Rotation::Clockwise90.rotate(Direction::North),
            Direction::East
        );
        assert_eq!(
            Rotation::Clockwise180.rotate(Direction::North),
            Direction::South
        );
        assert_eq!(
            Rotation::CounterClockwise90.rotate(Direction::North),
            Direction::West
        );
    }

    #[test]
    fn compose_rotations() {
        assert_eq!(
            Rotation::Clockwise90.then(Rotation::Clockwise90),
            Rotation::Clockwise180
        );
        assert_eq!(
            Rotation::Clockwise90.then(Rotation::CounterClockwise90),
            Rotation::None
        );
        assert_eq!(
            Rotation::Clockwise180.then(Rotation::Clockwise180),
            Rotation::None
        );
    }

    #[test]
    fn vertical_unchanged() {
        assert_eq!(Rotation::Clockwise90.rotate(Direction::Up), Direction::Up);
        assert_eq!(
            Rotation::Clockwise180.rotate(Direction::Down),
            Direction::Down
        );
    }

    #[test]
    fn transform_pos_pivot_zero() {
        assert_eq!(Rotation::None.transform_pos(3, 5, 7, 0, 0), (3, 5, 7));
        assert_eq!(
            Rotation::Clockwise90.transform_pos(3, 5, 7, 0, 0),
            (-7, 5, 3)
        );
        assert_eq!(
            Rotation::Clockwise180.transform_pos(3, 5, 7, 0, 0),
            (-3, 5, -7)
        );
        assert_eq!(
            Rotation::CounterClockwise90.transform_pos(3, 5, 7, 0, 0),
            (7, 5, -3)
        );
    }

    #[test]
    fn bounding_box_none() {
        let bb = Rotation::None.get_bounding_box(0, 0, 0, 6, 10, 6);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (0, 0, 0));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (5, 9, 5));
    }

    #[test]
    fn bounding_box_cw90() {
        let bb = Rotation::Clockwise90.get_bounding_box(100, 50, 200, 6, 10, 8);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (93, 50, 200));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (100, 59, 205));
    }

    #[test]
    fn bounding_box_cw180() {
        let bb = Rotation::Clockwise180.get_bounding_box(0, 0, 0, 6, 10, 8);
        assert_eq!((bb.min_x, bb.min_y, bb.min_z), (-5, 0, -7));
        assert_eq!((bb.max_x, bb.max_y, bb.max_z), (0, 9, 0));
    }

    #[test]
    fn rotate_size() {
        assert_eq!(Rotation::None.rotate_size(6, 10, 8), (6, 10, 8));
        assert_eq!(Rotation::Clockwise90.rotate_size(6, 10, 8), (8, 10, 6));
        assert_eq!(Rotation::Clockwise180.rotate_size(6, 10, 8), (6, 10, 8));
        assert_eq!(
            Rotation::CounterClockwise90.rotate_size(6, 10, 8),
            (8, 10, 6)
        );
    }
}
