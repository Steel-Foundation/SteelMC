//! Geometry primitives shared by registry data, physics, and world queries.

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Neg, Sub};

use glam::{DVec3, IVec3};
use glam_traits::GVec3;
use wincode::config::ConfigCore;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, TypeMeta, WriteResult};

use crate::{BlockPos, axis::Axis};

const fn ordered_pair(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Marker type for block-local AABBs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockLocal;

/// Marker type for world-space AABBs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct World;

/// Marker type for integer bounding boxes (structure pieces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Structure;

/// Generic axis-aligned bounding box.
///
/// `T` is the vector type (e.g. [`DVec3`] or [`IVec3`]) and `I` is a marker
/// that differentiates coordinate spaces (block-local, world, structure).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb<T, I> {
    /// Minimum corner of the box.
    pub min: T,
    /// Maximum corner of the box.
    pub max: T,
    p: PhantomData<I>,
}

/// Block-local axis-aligned box used by voxel shapes.
pub type BlockLocalAabb = Aabb<DVec3, BlockLocal>;

/// World-space axis-aligned box used by entity and collision physics.
pub type WorldAabb = Aabb<DVec3, World>;

/// Integer axis-aligned bounding box for structure pieces.
pub type BoundingBox = Aabb<IVec3, Structure>;

/// Integer axis-aligned bounding box for structure pieces but with wincode impl.
pub struct WincodeBoundingBox(pub BoundingBox);

// SAFETY: WincodeBoundingBox is a statically sized type (24 bytes, six i32 fields)
// with no invalid bit patterns when fully initialized. The implementations
// correctly serialize/deserialize all six components in order.
unsafe impl<C: ConfigCore> SchemaWrite<C> for WincodeBoundingBox {
    type Src = Self;

    const TYPE_META: TypeMeta = TypeMeta::Static {
        size: 6 * size_of::<i32>(),
        zero_copy: false,
    };

    fn size_of(_src: &Self::Src) -> WriteResult<usize> {
        Ok(6 * size_of::<i32>())
    }

    fn write(mut writer: impl Writer, src: &Self::Src) -> WriteResult<()> {
        let bbox = &src.0;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.min.x)?;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.min.y)?;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.min.z)?;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.max.x)?;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.max.y)?;
        <i32 as SchemaWrite<C>>::write(writer.by_ref(), &bbox.max.z)?;
        Ok(())
    }
}

// SAFETY: The implementation reads exactly six i32 values and constructs a valid
// WincodeBoundingBox, fully initialising the destination.
unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for WincodeBoundingBox {
    type Dst = Self;

    const TYPE_META: TypeMeta = TypeMeta::Static {
        size: 6 * size_of::<i32>(),
        zero_copy: false,
    };

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let mut min_x = MaybeUninit::uninit();
        let mut min_y = MaybeUninit::uninit();
        let mut min_z = MaybeUninit::uninit();
        let mut max_x = MaybeUninit::uninit();
        let mut max_y = MaybeUninit::uninit();
        let mut max_z = MaybeUninit::uninit();

        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut min_x)?;
        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut min_y)?;
        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut min_z)?;
        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut max_x)?;
        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut max_y)?;
        <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), &mut max_z)?;

        // SAFETY: Each `read` call above returned `Ok(())`, which guarantees the
        // corresponding `MaybeUninit` is fully initialised.
        let (min_x, min_y, min_z, max_x, max_y, max_z) = unsafe {
            (
                min_x.assume_init(),
                min_y.assume_init(),
                min_z.assume_init(),
                max_x.assume_init(),
                max_y.assume_init(),
                max_z.assume_init(),
            )
        };

        // SAFETY: `dst` is uninitialised at this point, and we are writing a valid,
        // fully constructed `WincodeBoundingBox` into it.
        unsafe {
            dst.as_mut_ptr().write(WincodeBoundingBox(BoundingBox {
                min: IVec3::new(min_x, min_y, min_z),
                max: IVec3::new(max_x, max_y, max_z),
                p: PhantomData,
            }));
        }

        Ok(())
    }
}

impl<T: GVec3, I> Aabb<T, I> {
    /// Returns the minimum coordinate on `axis`.
    pub fn min(&self, axis: Axis) -> T::Scalar {
        match axis {
            Axis::X => self.min.x(),
            Axis::Y => self.min.y(),
            Axis::Z => self.min.z(),
        }
    }

    /// Returns the maximum coordinate on `axis`.
    pub fn max(&self, axis: Axis) -> T::Scalar {
        match axis {
            Axis::X => self.max.x(),
            Axis::Y => self.max.y(),
            Axis::Z => self.max.z(),
        }
    }

    /// Returns `true` when this box has no positive volume on at least one axis.
    pub fn is_empty(&self) -> bool {
        self.min.x() >= self.max.x() || self.min.y() >= self.max.y() || self.min.z() >= self.max.z()
    }

    /// Creates an AABB ensuring min <= max on every axis.
    pub fn from_min_max(min: T, max: T) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
            p: PhantomData,
        }
    }

    /// Translates the box by a vector.
    #[must_use]
    pub fn translate(self, delta: T) -> Self {
        Self {
            min: self.min + delta,
            max: self.max + delta,
            p: PhantomData,
        }
    }

    /// Expands the box in every direction by `amount`.
    #[must_use]
    pub fn inflate(self, amount: T::Scalar) -> Self {
        self.inflate_xyz(amount, amount, amount)
    }

    /// Expands the box independently on each axis.
    #[must_use]
    pub fn inflate_xyz(self, x: T::Scalar, y: T::Scalar, z: T::Scalar) -> Self {
        let delta = T::new(x, y, z);
        Self {
            min: self.min - delta,
            max: self.max + delta,
            p: PhantomData,
        }
    }

    /// Returns the smallest AABB that contains both `a` and `b`.
    #[must_use]
    pub fn encapsulating(a: &Self, b: &Self) -> Self {
        Self {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
            p: PhantomData,
        }
    }
}

impl<T: GVec3, I> Aabb<T, I>
where
    T::Scalar: Neg<Output = T::Scalar>,
{
    /// Shrinks the box by `amount` in every direction.
    #[must_use]
    pub fn deflate(self, amount: T::Scalar) -> Self {
        self.inflate(-amount)
    }
}

impl<T: GVec3, I> Aabb<T, I>
where
    T::Scalar: Sub<Output = T::Scalar>,
{
    /// Get the width of bounding box
    pub fn width(&self) -> T::Scalar {
        self.max.x() - self.min.x()
    }

    /// Get the height of bounding box
    pub fn height(&self) -> T::Scalar {
        self.max.y() - self.min.y()
    }

    /// Get the depth of bounding box
    pub fn depth(&self) -> T::Scalar {
        self.max.z() - self.min.z()
    }
}

impl<I> Aabb<DVec3, I> {
    /// A full block from `(0, 0, 0)` to `(1, 1, 1)`.
    pub const FULL_BLOCK: Self = Self::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    /// A zero-volume box.
    pub const EMPTY: Self = Self::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    /// Creates an AABB and normalizes endpoint order like vanilla `AABB`.
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
            min: DVec3::new(min_x, min_y, min_z),
            max: DVec3::new(max_x, max_y, max_z),
            p: PhantomData,
        }
    }

    /// Vanilla equivalent: `AABB.getSize()`.
    #[must_use]
    pub fn size(self) -> f64 {
        (self.width() + self.height() + self.depth()) / 3.0
    }

    /// Returns `true` if this box intersects `other`.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.intersects_coords(other.min, other.max)
    }

    /// Returns `true` if this box intersects the given bounds.
    #[must_use]
    pub fn intersects_coords(self, min: DVec3, max: DVec3) -> bool {
        self.min.x < max.x
            && self.max.x > min.x
            && self.min.y < max.y
            && self.max.y > min.y
            && self.min.z < max.z
            && self.max.z > min.z
    }

    /// Returns `true` if the point lies inside this box.
    ///
    /// Maximum edges are exclusive, matching vanilla Minecraft behaviour.
    #[must_use]
    pub fn contains(self, x: f64, y: f64, z: f64) -> bool {
        x >= self.min.x
            && x < self.max.x
            && y >= self.min.y
            && y < self.max.y
            && z >= self.min.z
            && z < self.max.z
    }
}

impl Aabb<DVec3, BlockLocal> {
    /// Converts this block-local box to a world-space box at `pos`.
    #[must_use]
    pub fn at_block(self, pos: BlockPos) -> Aabb<DVec3, World> {
        let offset = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        Aabb {
            min: self.min + offset,
            max: self.max + offset,
            p: PhantomData,
        }
    }
}

impl Aabb<DVec3, World> {
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

    /// Expands the box only in the direction of `delta`.
    #[must_use]
    pub fn expand_towards(self, delta: DVec3) -> Self {
        Aabb {
            min: DVec3::new(
                if delta.x < 0.0 {
                    self.min.x + delta.x
                } else {
                    self.min.x
                },
                if delta.y < 0.0 {
                    self.min.y + delta.y
                } else {
                    self.min.y
                },
                if delta.z < 0.0 {
                    self.min.z + delta.z
                } else {
                    self.min.z
                },
            ),
            max: DVec3::new(
                if delta.x > 0.0 {
                    self.max.x + delta.x
                } else {
                    self.max.x
                },
                if delta.y > 0.0 {
                    self.max.y + delta.y
                } else {
                    self.max.y
                },
                if delta.z > 0.0 {
                    self.max.z + delta.z
                } else {
                    self.max.z
                },
            ),
            p: PhantomData,
        }
    }

    /// Returns `true` if this box intersects the full block at `pos`.
    #[must_use]
    pub fn intersects_block(self, pos: BlockPos) -> bool {
        let min = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        let max = min + DVec3::ONE;
        self.intersects_coords(min, max)
    }
}

impl Aabb<IVec3, Structure> {
    /// Creates a new bounding box, normalizing so min <= max on each axis.
    #[must_use]
    pub fn new(pos1: IVec3, pos2: IVec3) -> Self {
        Self::from_min_max(pos1, pos2)
    }

    /// Creates a bounding box from two corner block positions.
    #[must_use]
    pub fn from_corners(a: BlockPos, b: BlockPos) -> Self {
        Self::new(a.0, b.0)
    }

    /// Returns whether this bounding box intersects another.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.max.x >= other.min.x
            && self.min.x <= other.max.x
            && self.max.y >= other.min.y
            && self.min.y <= other.max.y
            && self.max.z >= other.min.z
            && self.min.z <= other.max.z
    }

    /// Returns whether this bounding box intersects the given XZ range.
    #[must_use]
    pub const fn intersects_xz(self, min_x: i32, min_z: i32, max_x: i32, max_z: i32) -> bool {
        self.max.x >= min_x && self.min.x <= max_x && self.max.z >= min_z && self.min.z <= max_z
    }

    /// Returns whether the given block position is inside this bounding box.
    #[must_use]
    pub const fn is_inside(self, pos: BlockPos) -> bool {
        self.contains_xyz(pos.x(), pos.y(), pos.z())
    }

    /// Returns whether the given coordinates are inside this bounding box.
    #[must_use]
    pub const fn contains_xyz(self, x: i32, y: i32, z: i32) -> bool {
        x >= self.min.x
            && x <= self.max.x
            && y >= self.min.y
            && y <= self.max.y
            && z >= self.min.z
            && z <= self.max.z
    }

    /// Returns the center block position of this bounding box.
    #[must_use]
    pub const fn get_center(self) -> BlockPos {
        BlockPos(IVec3::new(
            self.min.x + (self.max.x - self.min.x + 1) / 2,
            self.min.y + (self.max.y - self.min.y + 1) / 2,
            self.min.z + (self.max.z - self.min.z + 1) / 2,
        ))
    }

    /// Returns the span (size) along the X axis (number of blocks).
    #[must_use]
    pub const fn get_x_span(self) -> i32 {
        self.max.x - self.min.x + 1
    }
    /// Returns the span (size) along the Y axis.
    #[must_use]
    pub const fn get_y_span(self) -> i32 {
        self.max.y - self.min.y + 1
    }
    /// Returns the span (size) along the Z axis.
    #[must_use]
    pub const fn get_z_span(self) -> i32 {
        self.max.z - self.min.z + 1
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
        assert_eq!(aabb.min.x, 1.0);
        assert_eq!(aabb.min.y, 2.0);
        assert_eq!(aabb.min.z, 0.0);
        assert_eq!(aabb.max.x, 3.0);
        assert_eq!(aabb.max.y, 4.0);
        assert_eq!(aabb.max.z, 5.0);
    }

    #[test]
    fn block_local_aabb_translates_to_world_space() {
        let local = BlockLocalAabb::new(0.0, 0.25, 0.0, 1.0, 0.75, 1.0);
        let world = local.at_block(BlockPos::new(10, 64, -5));

        assert_eq!(world.min.x, 10.0);
        assert_eq!(world.min.y, 64.25);
        assert_eq!(world.min.z, -5.0);
        assert_eq!(world.max.x, 11.0);
        assert_eq!(world.max.y, 64.75);
        assert_eq!(world.max.z, -4.0);
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

        assert_eq!(swept.min.x, 0.5);
        assert_eq!(swept.min.y, 1.0);
        assert_eq!(swept.min.z, 1.0);
        assert_eq!(swept.max.x, 2.0);
        assert_eq!(swept.max.y, 3.5);
        assert_eq!(swept.max.z, 2.0);
    }
}
