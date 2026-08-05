//! Vanilla's `Rotation` — horizontal rotations around the Y axis.

use glam::IVec3;

use crate::Direction;
use crate::geometry::BoundingBox;
use crate::random::Random;

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
    pub fn get_random(rng: &mut impl Random) -> Self {
        ALL_ROTATIONS[rng.next_i32_bounded(4) as usize]
    }

    /// Matches vanilla's `Util.shuffledCopy(values(), random)` (reverse Fisher-Yates).
    #[must_use]
    pub fn get_shuffled(rng: &mut impl Random) -> [Rotation; 4] {
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
    ///
    /// `pivot.y` is ignored (only the XZ plane matters).
    #[must_use]
    pub const fn transform_pos(self, pos: IVec3, pivot: IVec3) -> IVec3 {
        let (x, y, z) = (pos.x, pos.y, pos.z);
        let (px, pz) = (pivot.x, pivot.z);
        match self {
            Self::None => IVec3::new(x, y, z),
            Self::Clockwise90 => IVec3::new(px + pz - z, y, pz - px + x),
            Self::Clockwise180 => IVec3::new(px + px - x, y, pz + pz - z),
            Self::CounterClockwise90 => IVec3::new(px - pz + z, y, px + pz - x),
        }
    }

    /// 90°/270° swap the X and Z dimensions.
    #[must_use]
    pub const fn rotate_size(self, size: IVec3) -> IVec3 {
        match self {
            Self::Clockwise90 | Self::CounterClockwise90 => IVec3::new(size.z, size.y, size.x),
            Self::None | Self::Clockwise180 => size,
        }
    }

    /// Matches vanilla's `StructureTemplate.transform(pos, Mirror.FRONT_BACK, rotation, pivot)`.
    #[must_use]
    pub const fn transform_pos_mirrored(
        self,
        pos: IVec3,
        pivot: IVec3,
        mirror_front_back: bool,
    ) -> IVec3 {
        let mx = if mirror_front_back { -pos.x } else { pos.x };
        let mirrored_pos = IVec3::new(mx, pos.y, pos.z);
        self.transform_pos(mirrored_pos, pivot)
    }

    /// Matches vanilla's `StructureTemplate.getBoundingBox(position, rotation, pivot, mirror, size)`.
    #[must_use]
    pub fn get_bounding_box_full(
        self,
        pos: IVec3,
        size: IVec3,
        pivot: IVec3,
        mirror_front_back: bool,
    ) -> BoundingBox {
        let c1 = self.transform_pos_mirrored(IVec3::ZERO, pivot, mirror_front_back);
        let c2 = self.transform_pos_mirrored(
            IVec3::new(size.x - 1, size.y - 1, size.z - 1),
            pivot,
            mirror_front_back,
        );
        BoundingBox::new(c1.min(c2) + pos, c1.max(c2) + pos)
    }

    /// [`get_bounding_box_full`] with `mirror=NONE`.
    #[must_use]
    pub fn get_bounding_box_with_pivot(self, pos: IVec3, size: IVec3, pivot: IVec3) -> BoundingBox {
        self.get_bounding_box_full(pos, size, pivot, false)
    }

    /// [`get_bounding_box_full`] with `pivot=ZERO` and `mirror=NONE`. Used by jigsaw pool elements.
    #[must_use]
    pub fn get_bounding_box(self, pos: IVec3, size: IVec3) -> BoundingBox {
        self.get_bounding_box_full(pos, size, IVec3::ZERO, false)
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
        assert_eq!(
            Rotation::None.transform_pos(IVec3::new(3, 5, 7), IVec3::ZERO),
            IVec3::new(3, 5, 7)
        );
        assert_eq!(
            Rotation::Clockwise90.transform_pos(IVec3::new(3, 5, 7), IVec3::ZERO),
            IVec3::new(-7, 5, 3)
        );
        assert_eq!(
            Rotation::Clockwise180.transform_pos(IVec3::new(3, 5, 7), IVec3::ZERO),
            IVec3::new(-3, 5, -7)
        );
        assert_eq!(
            Rotation::CounterClockwise90.transform_pos(IVec3::new(3, 5, 7), IVec3::ZERO),
            IVec3::new(7, 5, -3)
        );
    }

    #[test]
    fn bounding_box_none() {
        let bb = Rotation::None.get_bounding_box(IVec3::new(0, 0, 0), IVec3::new(6, 10, 6));
        assert_eq!((bb.min_x(), bb.min_y(), bb.min_z()), (0, 0, 0));
        assert_eq!((bb.max_x(), bb.max_y(), bb.max_z()), (5, 9, 5));
    }

    #[test]
    fn bounding_box_cw90() {
        let bb =
            Rotation::Clockwise90.get_bounding_box(IVec3::new(100, 50, 200), IVec3::new(6, 10, 8));
        assert_eq!((bb.min_x(), bb.min_y(), bb.min_z()), (93, 50, 200));
        assert_eq!((bb.max_x(), bb.max_y(), bb.max_z()), (100, 59, 205));
    }

    #[test]
    fn bounding_box_cw180() {
        let bb = Rotation::Clockwise180.get_bounding_box(IVec3::new(0, 0, 0), IVec3::new(6, 10, 8));
        assert_eq!((bb.min_x(), bb.min_y(), bb.min_z()), (-5, 0, -7));
        assert_eq!((bb.max_x(), bb.max_y(), bb.max_z()), (0, 9, 0));
    }

    #[test]
    fn rotate_size() {
        assert_eq!(
            Rotation::None.rotate_size(IVec3::new(6, 10, 8)),
            IVec3::new(6, 10, 8)
        );
        assert_eq!(
            Rotation::Clockwise90.rotate_size(IVec3::new(6, 10, 8)),
            IVec3::new(8, 10, 6)
        );
        assert_eq!(
            Rotation::Clockwise180.rotate_size(IVec3::new(6, 10, 8)),
            IVec3::new(6, 10, 8)
        );
        assert_eq!(
            Rotation::CounterClockwise90.rotate_size(IVec3::new(6, 10, 8)),
            IVec3::new(8, 10, 6)
        );
    }
}
