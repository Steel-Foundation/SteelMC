//! Geometry primitives shared by registry data, physics, and world queries.

use glam::DVec3;

use crate::{BlockPos, axis::Axis};

const fn ordered_pair(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Block-local axis-aligned box used by voxel shapes.
///
/// Coordinates are relative to a block position. Vanilla block shapes are
/// usually in 0.0..=1.0 space, but some shapes extend outside that range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockLocalAabb {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl BlockLocalAabb {
    /// A full block from `(0, 0, 0)` to `(1, 1, 1)`.
    pub const FULL_BLOCK: Self = Self::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    /// A zero-volume box. Empty voxel shapes should prefer an empty box slice.
    pub const EMPTY: Self = Self::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    /// Creates a block-local AABB and normalizes endpoint order like vanilla
    /// `AABB`.
    #[must_use]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        let (min_x, max_x) = ordered_pair(min_x, max_x);
        let (min_y, max_y) = ordered_pair(min_y, max_y);
        let (min_z, max_z) = ordered_pair(min_z, max_z);
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    #[must_use]
    /// Returns the minimum X coordinate.
    pub const fn min_x(self) -> f64 {
        self.min_x
    }

    #[must_use]
    /// Returns the minimum Y coordinate.
    pub const fn min_y(self) -> f64 {
        self.min_y
    }

    #[must_use]
    /// Returns the minimum Z coordinate.
    pub const fn min_z(self) -> f64 {
        self.min_z
    }

    #[must_use]
    /// Returns the maximum X coordinate.
    pub const fn max_x(self) -> f64 {
        self.max_x
    }

    #[must_use]
    /// Returns the maximum Y coordinate.
    pub const fn max_y(self) -> f64 {
        self.max_y
    }

    #[must_use]
    /// Returns the maximum Z coordinate.
    pub const fn max_z(self) -> f64 {
        self.max_z
    }

    #[must_use]
    /// Returns the minimum coordinate on `axis`.
    pub const fn min(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.min_x,
            Axis::Y => self.min_y,
            Axis::Z => self.min_z,
        }
    }

    #[must_use]
    /// Returns the maximum coordinate on `axis`.
    pub const fn max(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.max_x,
            Axis::Y => self.max_y,
            Axis::Z => self.max_z,
        }
    }

    /// Returns true when this box has no positive volume on at least one axis.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y || self.min_z >= self.max_z
    }

    #[must_use]
    /// Returns the X size.
    pub fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    #[must_use]
    /// Returns the Y size.
    pub fn height(self) -> f64 {
        self.max_y - self.min_y
    }

    #[must_use]
    /// Returns the Z size.
    pub fn depth(self) -> f64 {
        self.max_z - self.min_z
    }

    /// Vanilla equivalent: `AABB.getSize()`.
    #[must_use]
    pub fn size(self) -> f64 {
        (self.width() + self.height() + self.depth()) / 3.0
    }

    #[must_use]
    /// Returns this box translated by the given delta.
    pub fn move_by(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x + dx,
            self.min_y + dy,
            self.min_z + dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    #[must_use]
    /// Returns this box expanded by `amount` in every direction.
    pub fn inflate(self, amount: f64) -> Self {
        self.inflate_xyz(amount, amount, amount)
    }

    #[must_use]
    /// Returns this box expanded independently on each axis.
    pub fn inflate_xyz(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x - x,
            self.min_y - y,
            self.min_z - z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    #[must_use]
    /// Returns this box shrunk by `amount` in every direction.
    pub fn deflate(self, amount: f64) -> Self {
        self.inflate(-amount)
    }

    #[must_use]
    /// Returns true if this box intersects `other`.
    pub fn intersects(self, other: Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }

    #[must_use]
    /// Returns true if the point lies inside this box.
    pub fn contains(self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }

    /// Converts this block-local box to a world-space box at `pos`.
    #[must_use]
    pub fn at_block(self, pos: BlockPos) -> WorldAabb {
        let x = f64::from(pos.x());
        let y = f64::from(pos.y());
        let z = f64::from(pos.z());
        WorldAabb::new(
            x + self.min_x,
            y + self.min_y,
            z + self.min_z,
            x + self.max_x,
            y + self.max_y,
            z + self.max_z,
        )
    }
}

/// World-space axis-aligned box used by entity and collision physics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldAabb {
    min_x: f64,
    min_y: f64,
    min_z: f64,
    max_x: f64,
    max_y: f64,
    max_z: f64,
}

impl WorldAabb {
    /// Creates a world-space AABB and normalizes endpoint order like vanilla
    /// `AABB`.
    #[must_use]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        let (min_x, max_x) = ordered_pair(min_x, max_x);
        let (min_y, max_y) = ordered_pair(min_y, max_y);
        let (min_z, max_z) = ordered_pair(min_z, max_z);
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// Creates an entity bounding box centered on X/Z and using `y` as feet.
    #[must_use]
    pub fn entity_box(x: f64, y: f64, z: f64, half_width: f64, height: f64) -> Self {
        Self::new(
            x - half_width,
            y,
            z - half_width,
            x + half_width,
            y + height,
            z + half_width,
        )
    }

    #[must_use]
    /// Returns the minimum X coordinate.
    pub const fn min_x(self) -> f64 {
        self.min_x
    }

    #[must_use]
    /// Returns the minimum Y coordinate.
    pub const fn min_y(self) -> f64 {
        self.min_y
    }

    #[must_use]
    /// Returns the minimum Z coordinate.
    pub const fn min_z(self) -> f64 {
        self.min_z
    }

    #[must_use]
    /// Returns the maximum X coordinate.
    pub const fn max_x(self) -> f64 {
        self.max_x
    }

    #[must_use]
    /// Returns the maximum Y coordinate.
    pub const fn max_y(self) -> f64 {
        self.max_y
    }

    #[must_use]
    /// Returns the maximum Z coordinate.
    pub const fn max_z(self) -> f64 {
        self.max_z
    }

    #[must_use]
    /// Returns the minimum coordinate on `axis`.
    pub const fn min(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.min_x,
            Axis::Y => self.min_y,
            Axis::Z => self.min_z,
        }
    }

    #[must_use]
    /// Returns the maximum coordinate on `axis`.
    pub const fn max(self, axis: Axis) -> f64 {
        match axis {
            Axis::X => self.max_x,
            Axis::Y => self.max_y,
            Axis::Z => self.max_z,
        }
    }

    #[must_use]
    /// Returns true when this box has no positive volume on at least one axis.
    pub const fn is_empty(self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y || self.min_z >= self.max_z
    }

    #[must_use]
    /// Returns the X size.
    pub fn width(self) -> f64 {
        self.max_x - self.min_x
    }

    #[must_use]
    /// Returns the Y size.
    pub fn height(self) -> f64 {
        self.max_y - self.min_y
    }

    #[must_use]
    /// Returns the Z size.
    pub fn depth(self) -> f64 {
        self.max_z - self.min_z
    }

    #[must_use]
    /// Vanilla equivalent: `AABB.getSize()`.
    pub fn size(self) -> f64 {
        (self.width() + self.height() + self.depth()) / 3.0
    }

    #[must_use]
    /// Returns this box translated by the given delta.
    pub fn move_by(self, dx: f64, dy: f64, dz: f64) -> Self {
        Self::new(
            self.min_x + dx,
            self.min_y + dy,
            self.min_z + dz,
            self.max_x + dx,
            self.max_y + dy,
            self.max_z + dz,
        )
    }

    #[must_use]
    /// Returns this box translated by `delta`.
    pub fn move_vec(self, delta: DVec3) -> Self {
        self.move_by(delta.x, delta.y, delta.z)
    }

    #[must_use]
    /// Returns this box expanded by `amount` in every direction.
    pub fn inflate(self, amount: f64) -> Self {
        self.inflate_xyz(amount, amount, amount)
    }

    #[must_use]
    /// Returns this box expanded independently on each axis.
    pub fn inflate_xyz(self, x: f64, y: f64, z: f64) -> Self {
        Self::new(
            self.min_x - x,
            self.min_y - y,
            self.min_z - z,
            self.max_x + x,
            self.max_y + y,
            self.max_z + z,
        )
    }

    #[must_use]
    /// Returns this box shrunk by `amount` in every direction.
    pub fn deflate(self, amount: f64) -> Self {
        self.inflate(-amount)
    }

    #[must_use]
    /// Returns this box expanded only in the direction of `delta`.
    pub fn expand_towards(self, delta: DVec3) -> Self {
        Self::new(
            if delta.x < 0.0 {
                self.min_x + delta.x
            } else {
                self.min_x
            },
            if delta.y < 0.0 {
                self.min_y + delta.y
            } else {
                self.min_y
            },
            if delta.z < 0.0 {
                self.min_z + delta.z
            } else {
                self.min_z
            },
            if delta.x > 0.0 {
                self.max_x + delta.x
            } else {
                self.max_x
            },
            if delta.y > 0.0 {
                self.max_y + delta.y
            } else {
                self.max_y
            },
            if delta.z > 0.0 {
                self.max_z + delta.z
            } else {
                self.max_z
            },
        )
    }

    #[must_use]
    /// Returns true if this box intersects `other`.
    pub fn intersects(self, other: Self) -> bool {
        self.intersects_coords(
            other.min_x,
            other.min_y,
            other.min_z,
            other.max_x,
            other.max_y,
            other.max_z,
        )
    }

    #[must_use]
    /// Returns true if this box intersects the given raw coordinate bounds.
    pub fn intersects_coords(
        self,
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> bool {
        self.min_x < max_x
            && self.max_x > min_x
            && self.min_y < max_y
            && self.max_y > min_y
            && self.min_z < max_z
            && self.max_z > min_z
    }

    #[must_use]
    /// Returns true if this box intersects the full block at `pos`.
    pub fn intersects_block(self, pos: BlockPos) -> bool {
        self.intersects_coords(
            f64::from(pos.x()),
            f64::from(pos.y()),
            f64::from(pos.z()),
            f64::from(pos.x()) + 1.0,
            f64::from(pos.y()) + 1.0,
            f64::from(pos.z()) + 1.0,
        )
    }

    #[must_use]
    /// Returns true if the point lies inside this box.
    pub fn contains(self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min_x
            && x < self.max_x
            && y >= self.min_y
            && y < self.max_y
            && z >= self.min_z
            && z < self.max_z
    }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "geometry constructors use exact test values"
)]
mod tests {
    use super::*;

    #[test]
    fn constructors_normalize_endpoints_like_vanilla() {
        let aabb = WorldAabb::new(3.0, 4.0, 5.0, 1.0, 2.0, 0.0);
        assert_eq!(aabb.min_x(), 1.0);
        assert_eq!(aabb.min_y(), 2.0);
        assert_eq!(aabb.min_z(), 0.0);
        assert_eq!(aabb.max_x(), 3.0);
        assert_eq!(aabb.max_y(), 4.0);
        assert_eq!(aabb.max_z(), 5.0);
    }

    #[test]
    fn block_local_aabb_translates_to_world_space() {
        let local = BlockLocalAabb::new(0.0, 0.25, 0.0, 1.0, 0.75, 1.0);
        let world = local.at_block(BlockPos::new(10, 64, -5));

        assert_eq!(world.min_x(), 10.0);
        assert_eq!(world.min_y(), 64.25);
        assert_eq!(world.min_z(), -5.0);
        assert_eq!(world.max_x(), 11.0);
        assert_eq!(world.max_y(), 64.75);
        assert_eq!(world.max_z(), -4.0);
    }

    #[test]
    fn contains_uses_vanilla_exclusive_max_edge() {
        let aabb = WorldAabb::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

        assert!(aabb.contains(0.0, 0.5, 0.5));
        assert!(aabb.contains(0.999, 0.5, 0.5));
        assert!(!aabb.contains(1.0, 0.5, 0.5));
    }

    #[test]
    fn expand_towards_covers_start_and_end() {
        let aabb = WorldAabb::new(1.0, 1.0, 1.0, 2.0, 2.0, 2.0);
        let swept = aabb.expand_towards(DVec3::new(-0.5, 1.5, 0.0));

        assert_eq!(swept.min_x(), 0.5);
        assert_eq!(swept.min_y(), 1.0);
        assert_eq!(swept.min_z(), 1.0);
        assert_eq!(swept.max_x(), 2.0);
        assert_eq!(swept.max_y(), 3.5);
        assert_eq!(swept.max_z(), 2.0);
    }
}
