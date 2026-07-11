//! Spatial index for jigsaw piece bounds. Port of `StructureLayoutOptimizer`'s
//! `BoxOctree` — nearby-box queries instead of scanning every placed piece.

use std::mem;

use glam::IVec3;
use steel_utils::BoundingBox;

const SUBDIVIDE_THRESHOLD: usize = 10;
const MAXIMUM_DEPTH: u32 = 3;

/// Vanilla `AABB.of(BoundingBox).deflate(0.25)` in quarter-block fixed point.
///
/// Piece AABB uses `[min, max + 1)`; deflate shrinks each axis by `0.25` on both
/// sides → `[min + 0.25, max + 0.75]`, encoded as `[min * 4 + 1, max * 4 + 3]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DeflatedQuarters {
    min_x: i32,
    min_y: i32,
    min_z: i32,
    max_x: i32,
    max_y: i32,
    max_z: i32,
}

impl DeflatedQuarters {
    #[inline]
    pub(crate) const fn from_piece(bbox: BoundingBox) -> Self {
        Self {
            min_x: bbox.min_x() * 4 + 1,
            min_y: bbox.min_y() * 4 + 1,
            min_z: bbox.min_z() * 4 + 1,
            max_x: bbox.max_x() * 4 + 3,
            max_y: bbox.max_y() * 4 + 3,
            max_z: bbox.max_z() * 4 + 3,
        }
    }

    /// Vanilla boundary AABB is `[min, max + 1)` without deflate.
    #[inline]
    pub(crate) const fn boundary_from(bbox: BoundingBox) -> Self {
        Self {
            min_x: bbox.min_x() * 4,
            min_y: bbox.min_y() * 4,
            min_z: bbox.min_z() * 4,
            max_x: bbox.max_x() * 4 + 4,
            max_y: bbox.max_y() * 4 + 4,
            max_z: bbox.max_z() * 4 + 4,
        }
    }

    #[inline]
    pub(crate) const fn is_empty(self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y || self.min_z >= self.max_z
    }

    #[inline]
    pub(crate) const fn contains(self, inner: Self) -> bool {
        self.min_x <= inner.min_x
            && self.min_y <= inner.min_y
            && self.min_z <= inner.min_z
            && self.max_x >= inner.max_x
            && self.max_y >= inner.max_y
            && self.max_z >= inner.max_z
    }

    #[inline]
    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.min_x < other.max_x
            && self.max_x > other.min_x
            && self.min_y < other.max_y
            && self.max_y > other.min_y
            && self.min_z < other.max_z
            && self.max_z > other.min_z
    }
}

/// Octree of axis-aligned boxes for fast intersection queries during jigsaw assembly.
#[derive(Debug, Clone)]
pub struct BoxOctree {
    boundary: BoundingBox,
    boundary_quarters: DeflatedQuarters,
    size: IVec3,
    depth: u32,
    inner_boxes: Vec<StoredBox>,
    children: Vec<BoxOctree>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StoredBox {
    bbox: BoundingBox,
    deflated: DeflatedQuarters,
}

impl StoredBox {
    const fn new(bbox: BoundingBox) -> Self {
        Self {
            bbox,
            deflated: DeflatedQuarters::from_piece(bbox),
        }
    }
}

impl BoxOctree {
    #[must_use]
    pub fn new(boundary: BoundingBox) -> Self {
        Self::with_depth(boundary, DeflatedQuarters::boundary_from(boundary), 0)
    }

    fn with_depth(
        boundary: BoundingBox,
        boundary_quarters: DeflatedQuarters,
        parent_depth: u32,
    ) -> Self {
        let size = IVec3::new(
            round_away_from_zero(boundary.width()),
            round_away_from_zero(boundary.height()),
            round_away_from_zero(boundary.depth()),
        );
        Self {
            boundary,
            boundary_quarters,
            size,
            depth: parent_depth + 1,
            inner_boxes: Vec::new(),
            children: Vec::new(),
        }
    }

    #[inline]
    pub fn add_box(&mut self, bbox: BoundingBox) {
        if self.depth < MAXIMUM_DEPTH && self.inner_boxes.len() > SUBDIVIDE_THRESHOLD {
            self.subdivide();
        }

        if !self.children.is_empty() {
            for child in &mut self.children {
                if child.boundary_intersects(bbox) {
                    child.add_box(bbox);
                }
            }
            return;
        }

        if self.inner_boxes.iter().any(|stored| stored.bbox == bbox) {
            return;
        }
        self.inner_boxes.push(StoredBox::new(bbox));
    }

    /// Vanilla jigsaw placement uses `AABB.of(bb).deflate(0.25)` before collision checks.
    #[inline]
    pub fn within_bounds_but_not_intersecting_children(&self, candidate: BoundingBox) -> bool {
        let deflated = DeflatedQuarters::from_piece(candidate);
        if deflated.is_empty() {
            return false;
        }
        self.boundary_quarters.contains(deflated) && !self.intersects_deflated(deflated, candidate)
    }

    #[inline]
    fn intersects_deflated(&self, deflated: DeflatedQuarters, candidate: BoundingBox) -> bool {
        if !self.children.is_empty() {
            return self.children.iter().any(|child| {
                child.boundary_quarters.intersects(deflated)
                    && child.intersects_deflated(deflated, candidate)
            });
        }
        self.inner_boxes
            .iter()
            .any(|stored| candidate.intersects(stored.bbox) && deflated.intersects(stored.deflated))
    }

    #[inline]
    fn boundary_intersects(&self, candidate: BoundingBox) -> bool {
        self.boundary.intersects(candidate)
    }

    fn subdivide(&mut self) {
        assert!(
            self.children.is_empty(),
            "BoxOctree: tried to subdivide when children already exist"
        );

        let min = self.boundary.min_corner();
        let max = self.boundary.max_corner();
        let half_x = self.size.x / 2;
        let half_y = self.size.y / 2;
        let half_z = self.size.z / 2;

        let child_bounds = [
            BoundingBox::new(
                IVec3::new(min.x, min.y, min.z),
                IVec3::new(min.x + half_x, min.y + half_y, min.z + half_z),
            ),
            BoundingBox::new(
                IVec3::new(min.x, min.y, min.z + half_z),
                IVec3::new(min.x + half_x, min.y + half_y, max.z),
            ),
            BoundingBox::new(
                IVec3::new(min.x + half_x, min.y, min.z),
                IVec3::new(max.x, min.y + half_y, min.z + half_z),
            ),
            BoundingBox::new(
                IVec3::new(min.x + half_x, min.y, min.z + half_z),
                IVec3::new(max.x, min.y + half_y, max.z),
            ),
            BoundingBox::new(
                IVec3::new(min.x, min.y + half_y, min.z),
                IVec3::new(min.x + half_x, max.y, min.z + half_z),
            ),
            BoundingBox::new(
                IVec3::new(min.x, min.y + half_y, min.z + half_z),
                IVec3::new(min.x + half_x, max.y, max.z),
            ),
            BoundingBox::new(
                IVec3::new(min.x + half_x, min.y + half_y, min.z),
                IVec3::new(max.x, max.y, min.z + half_z),
            ),
            BoundingBox::new(
                IVec3::new(min.x + half_x, min.y + half_y, min.z + half_z),
                IVec3::new(max.x, max.y, max.z),
            ),
        ];

        self.children = child_bounds
            .into_iter()
            .map(|boundary| {
                Self::with_depth(
                    boundary,
                    DeflatedQuarters::boundary_from(boundary),
                    self.depth,
                )
            })
            .collect();

        let inner_boxes = mem::take(&mut self.inner_boxes);
        for stored in inner_boxes {
            for child in &mut self.children {
                if child.boundary_intersects(stored.bbox) {
                    child.add_box(stored.bbox);
                }
            }
        }
    }
}

const fn round_away_from_zero(value: i32) -> i32 {
    if value >= 0 { value } else { -value }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_boxes_do_not_collide() {
        let boundary = BoundingBox::new(IVec3::ZERO, IVec3::new(100, 100, 100));
        let mut tree = BoxOctree::new(boundary);
        tree.add_box(BoundingBox::new(IVec3::ZERO, IVec3::new(5, 5, 5)));
        tree.add_box(BoundingBox::new(
            IVec3::new(50, 50, 50),
            IVec3::new(55, 55, 55),
        ));

        let candidate = BoundingBox::new(IVec3::new(10, 10, 10), IVec3::new(15, 15, 15));
        assert!(tree.within_bounds_but_not_intersecting_children(candidate));
    }

    #[test]
    fn nearby_boxes_collide() {
        let boundary = BoundingBox::new(IVec3::ZERO, IVec3::new(100, 100, 100));
        let mut tree = BoxOctree::new(boundary);
        tree.add_box(BoundingBox::new(IVec3::ZERO, IVec3::new(5, 5, 5)));

        let candidate = BoundingBox::new(IVec3::new(4, 4, 4), IVec3::new(8, 8, 8));
        assert!(!tree.within_bounds_but_not_intersecting_children(candidate));
    }
}
