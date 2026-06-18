//! Geometry primitives shared by registry data, physics, and world queries.

use std::marker::PhantomData;
use std::mem::MaybeUninit;
use std::ops::{Add, Div, Neg, Sub};

use glam::{DVec3, IVec3};
use wincode::config::ConfigCore;
use wincode::io::{Reader, Writer};
use wincode::{ReadResult, SchemaRead, SchemaWrite, TypeMeta, WriteResult};

use crate::{BlockPos, axis::Axis};

const fn ordered_pair(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Encodes the edge semantics of a coordinate space.
pub trait Space {
    /// Whether `min == max` on an axis means the box has zero extent.
    const ZERO_SPAN_IS_EMPTY: bool;
}

/// Marker type for block-local AABBs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockLocal;

impl Space for BlockLocal {
    const ZERO_SPAN_IS_EMPTY: bool = true;
}

/// Marker type for world-space AABBs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct World;

impl Space for World {
    const ZERO_SPAN_IS_EMPTY: bool = true;
}

/// Marker type for integer bounding boxes (structure pieces).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Structure;

impl Space for Structure {
    const ZERO_SPAN_IS_EMPTY: bool = false;
}

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

/// Vector operations used by generic AABB helpers.
pub trait AabbVector: Copy + Add<Output = Self> + Sub<Output = Self> {
    /// Scalar component type.
    type Scalar: Copy
        + PartialOrd
        + Add<Output = Self::Scalar>
        + Sub<Output = Self::Scalar>
        + Div<Output = Self::Scalar>
        + Neg<Output = Self::Scalar>
        + From<u8>;

    /// Creates a vector from individual components.
    fn new(x: Self::Scalar, y: Self::Scalar, z: Self::Scalar) -> Self;

    /// X component.
    fn x(self) -> Self::Scalar;

    /// Y component.
    fn y(self) -> Self::Scalar;

    /// Z component.
    fn z(self) -> Self::Scalar;

    /// Per-component minimum.
    #[must_use]
    fn min(self, other: Self) -> Self;

    /// Per-component maximum.
    #[must_use]
    fn max(self, other: Self) -> Self;
}

impl AabbVector for DVec3 {
    type Scalar = f64;

    fn new(x: Self::Scalar, y: Self::Scalar, z: Self::Scalar) -> Self {
        DVec3::new(x, y, z)
    }

    fn x(self) -> Self::Scalar {
        self.x
    }

    fn y(self) -> Self::Scalar {
        self.y
    }

    fn z(self) -> Self::Scalar {
        self.z
    }

    fn min(self, other: Self) -> Self {
        self.min(other)
    }

    fn max(self, other: Self) -> Self {
        self.max(other)
    }
}

impl AabbVector for IVec3 {
    type Scalar = i32;

    fn new(x: Self::Scalar, y: Self::Scalar, z: Self::Scalar) -> Self {
        IVec3::new(x, y, z)
    }

    fn x(self) -> Self::Scalar {
        self.x
    }

    fn y(self) -> Self::Scalar {
        self.y
    }

    fn z(self) -> Self::Scalar {
        self.z
    }

    fn min(self, other: Self) -> Self {
        self.min(other)
    }

    fn max(self, other: Self) -> Self {
        self.max(other)
    }
}

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
        for &val in &[
            bbox.min.x, bbox.min.y, bbox.min.z, bbox.max.x, bbox.max.y, bbox.max.z,
        ] {
            <i32 as SchemaWrite<C>>::write(writer.by_ref(), &val)?;
        }
        Ok(())
    }
}

// SAFETY: The implementation reads exactly six i32 values and constructs a valid
// WincodeBoundingBox, fully initializing the destination.
unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for WincodeBoundingBox {
    type Dst = Self;

    const TYPE_META: TypeMeta = TypeMeta::Static {
        size: 6 * size_of::<i32>(),
        zero_copy: false,
    };

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self::Dst>) -> ReadResult<()> {
        let mut vals = [MaybeUninit::<i32>::uninit(); 6];
        for slot in &mut vals {
            <i32 as SchemaRead<'de, C>>::read(reader.by_ref(), slot)?;
        }

        // SAFETY: all six reads succeeded, every slot is initialized.
        let [min_x, min_y, min_z, max_x, max_y, max_z] = vals.map(|v| unsafe { v.assume_init() });

        // SAFETY: dst is uninitialised; we write a fully-constructed value.
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

impl<T: AabbVector, I> Aabb<T, I> {
    /// Returns the minimum coordinate on `axis`.
    #[must_use]
    pub fn min(&self, axis: Axis) -> T::Scalar {
        match axis {
            Axis::X => self.min.x(),
            Axis::Y => self.min.y(),
            Axis::Z => self.min.z(),
        }
    }

    /// Returns the maximum coordinate on `axis`.
    #[must_use]
    pub fn max(&self, axis: Axis) -> T::Scalar {
        match axis {
            Axis::X => self.max.x(),
            Axis::Y => self.max.y(),
            Axis::Z => self.max.z(),
        }
    }

    /// Creates an AABB ensuring min <= max on every axis.
    #[must_use]
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

impl<T: AabbVector, I> Aabb<T, I> {
    /// Shrinks the box by `amount` in every direction.
    #[must_use]
    pub fn deflate(self, amount: T::Scalar) -> Self {
        self.inflate(-amount)
    }
}

impl<T: AabbVector, I: Space> Aabb<T, I> {
    #[inline]
    fn axis_overlaps(min1: T::Scalar, max1: T::Scalar, min2: T::Scalar, max2: T::Scalar) -> bool {
        if I::ZERO_SPAN_IS_EMPTY {
            min1 < max2 && max1 > min2
        } else {
            min1 <= max2 && max1 >= min2
        }
    }

    #[inline]
    fn axis_contains(min: T::Scalar, max: T::Scalar, v: T::Scalar) -> bool {
        if I::ZERO_SPAN_IS_EMPTY {
            v >= min && v < max
        } else {
            v >= min && v <= max
        }
    }

    /// Returns `true` when this box has no positive volume on at least one axis.
    pub fn is_empty(&self) -> bool {
        if I::ZERO_SPAN_IS_EMPTY {
            self.min.x() >= self.max.x()
                || self.min.y() >= self.max.y()
                || self.min.z() >= self.max.z()
        } else {
            self.min.x() > self.max.x()
                || self.min.y() > self.max.y()
                || self.min.z() > self.max.z()
        }
    }

    /// Returns whether this bounding box intersects another.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.intersects_bounds(other.min, other.max)
    }

    /// Returns `true` if this box intersects the given bounds.
    #[must_use]
    pub fn intersects_bounds(self, min: T, max: T) -> bool {
        Self::axis_overlaps(self.min.x(), self.max.x(), min.x(), max.x())
            && Self::axis_overlaps(self.min.y(), self.max.y(), min.y(), max.y())
            && Self::axis_overlaps(self.min.z(), self.max.z(), min.z(), max.z())
    }

    /// Returns whether this bounding box intersects the given XZ range.
    #[must_use]
    pub fn intersects_xz(
        self,
        min_x: T::Scalar,
        min_z: T::Scalar,
        max_x: T::Scalar,
        max_z: T::Scalar,
    ) -> bool {
        Self::axis_overlaps(self.min.x(), self.max.x(), min_x, max_x)
            && Self::axis_overlaps(self.min.z(), self.max.z(), min_z, max_z)
    }

    /// Returns whether the given coordinates are inside this bounding box.
    #[must_use]
    pub fn contains(self, pos: T) -> bool {
        self.contains_xyz(pos.x(), pos.y(), pos.z())
    }

    /// Returns whether the given coordinates are inside this bounding box.
    #[must_use]
    pub fn contains_xyz(self, x: T::Scalar, y: T::Scalar, z: T::Scalar) -> bool {
        Self::axis_contains(self.min.x(), self.max.x(), x)
            && Self::axis_contains(self.min.y(), self.max.y(), y)
            && Self::axis_contains(self.min.z(), self.max.z(), z)
    }
}

impl<T: AabbVector, I: Space> Aabb<T, I> {
    #[inline]
    fn span(raw: T::Scalar) -> T::Scalar {
        if I::ZERO_SPAN_IS_EMPTY {
            raw
        } else {
            raw + T::Scalar::from(1u8)
        }
    }

    /// Get the width of bounding box (X Span)
    #[must_use]
    pub fn width(&self) -> T::Scalar {
        Self::span(self.max.x() - self.min.x())
    }

    /// Get the height of bounding box (Y Span)
    #[must_use]
    pub fn height(&self) -> T::Scalar {
        Self::span(self.max.y() - self.min.y())
    }

    /// Get the depth of bounding box (Z Span)
    #[must_use]
    pub fn depth(&self) -> T::Scalar {
        Self::span(self.max.z() - self.min.z())
    }

    /// Returns the center point of the bounding box.
    #[must_use]
    pub fn center(&self) -> T {
        let two = T::Scalar::from(2u8);
        T::new(
            self.min.x() + Self::span(self.max.x() - self.min.x()) / two,
            self.min.y() + Self::span(self.max.y() - self.min.y()) / two,
            self.min.z() + Self::span(self.max.z() - self.min.z()) / two,
        )
    }
}

impl<I: Space> Aabb<IVec3, I> {
    /// Returns whether the given coordinates are inside this bounding box.
    #[must_use]
    pub fn contains_blockpos(self, pos: BlockPos) -> bool {
        self.contains(pos.0)
    }
}

impl<I: Space> Aabb<DVec3, I> {
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
        Self {
            min: self.min + delta.min(DVec3::ZERO),
            max: self.max + delta.max(DVec3::ZERO),
            p: PhantomData,
        }
    }

    /// Returns `true` if this box intersects the full block at `pos`.
    #[must_use]
    pub fn intersects_block(self, pos: BlockPos) -> bool {
        let min = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        let max = min + DVec3::ONE;
        self.intersects_bounds(min, max)
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

        assert!(aabb.contains_xyz(0.0, 0.5, 0.5));
        assert!(aabb.contains_xyz(0.999, 0.5, 0.5));
        assert!(!aabb.contains_xyz(1.0, 0.5, 0.5));
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
