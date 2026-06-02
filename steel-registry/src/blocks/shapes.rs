use steel_utils::BlockLocalAabb;

/// A block-local voxel shape.
///
/// This currently stores the optimized AABB list extracted from vanilla data.
/// It is intentionally a domain type rather than a raw slice so the full
/// vanilla shape implementation can grow behind the same API.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoxelShape {
    boxes: &'static [BlockLocalAabb],
}

impl VoxelShape {
    /// Empty shape.
    pub const EMPTY: Self = Self::from_boxes(&[]);

    /// Full block shape.
    pub const FULL_BLOCK: Self = Self::from_boxes(FULL_BLOCK_BOXES);

    /// Creates a shape from static block-local boxes.
    #[must_use]
    pub const fn from_boxes(boxes: &'static [BlockLocalAabb]) -> Self {
        Self { boxes }
    }

    /// Returns the block-local boxes backing this shape.
    #[must_use]
    pub const fn boxes(self) -> &'static [BlockLocalAabb] {
        self.boxes
    }

    /// Returns an iterator over the block-local boxes.
    pub fn iter(self) -> core::slice::Iter<'static, BlockLocalAabb> {
        self.boxes.iter()
    }

    /// Returns the number of block-local boxes in this shape.
    #[must_use]
    pub const fn len(self) -> usize {
        self.boxes.len()
    }

    /// Returns true if this shape has no boxes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.boxes.is_empty()
    }

    /// Returns the union bounds of this shape, or `None` for empty shapes.
    #[must_use]
    pub fn bounds(self) -> Option<BlockLocalAabb> {
        let (first, rest) = self.boxes.split_first()?;
        let mut min_x = first.min_x();
        let mut min_y = first.min_y();
        let mut min_z = first.min_z();
        let mut max_x = first.max_x();
        let mut max_y = first.max_y();
        let mut max_z = first.max_z();

        for aabb in rest {
            min_x = min_x.min(aabb.min_x());
            min_y = min_y.min(aabb.min_y());
            min_z = min_z.min(aabb.min_z());
            max_x = max_x.max(aabb.max_x());
            max_y = max_y.max(aabb.max_y());
            max_z = max_z.max(aabb.max_z());
        }

        Some(BlockLocalAabb::new(
            min_x, min_y, min_z, max_x, max_y, max_z,
        ))
    }
}

impl IntoIterator for VoxelShape {
    type IntoIter = core::slice::Iter<'static, BlockLocalAabb>;
    type Item = &'static BlockLocalAabb;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An ID referencing a registered VoxelShape in the ShapeRegistry.
///
/// Use this to refer to shapes in a compact way. The actual shape data
/// can be retrieved from the ShapeRegistry using this ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShapeId(pub u16);

impl ShapeId {
    /// The empty shape (no AABBs).
    pub const EMPTY: ShapeId = ShapeId(0);

    /// A full block shape.
    pub const FULL_BLOCK: ShapeId = ShapeId(1);
}

/// Registry for VoxelShapes.
///
/// Shapes are registered once and referenced by ShapeId. This allows
/// deduplication of shapes and compact storage of shape references.
///
/// Vanilla shapes are registered at startup. Plugins can register
/// additional shapes for custom blocks.
pub struct ShapeRegistry {
    shapes: Vec<VoxelShape>,
    allows_registering: bool,
}

impl Default for ShapeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ShapeRegistry {
    /// Creates a new shape registry with the standard empty and full block shapes.
    #[must_use]
    pub fn new() -> Self {
        let mut registry = Self {
            shapes: Vec::new(),
            allows_registering: true,
        };

        // Register the two standard shapes - IDs must match ShapeId::EMPTY and ShapeId::FULL_BLOCK
        let empty_id = registry.register(VoxelShape::EMPTY);
        debug_assert_eq!(empty_id, ShapeId::EMPTY);

        let full_id = registry.register(VoxelShape::FULL_BLOCK);
        debug_assert_eq!(full_id, ShapeId::FULL_BLOCK);

        registry
    }

    /// Registers a new shape and returns its ID.
    ///
    /// # Panics
    /// Panics if the registry has been frozen.
    pub fn register(&mut self, shape: VoxelShape) -> ShapeId {
        assert!(
            self.allows_registering,
            "Cannot register shapes after the registry has been frozen"
        );

        let id = ShapeId(self.shapes.len() as u16);
        self.shapes.push(shape);
        id
    }

    /// Gets the shape for a given ID.
    ///
    /// Returns an empty shape if the ID is invalid.
    #[must_use]
    pub fn get(&self, id: ShapeId) -> VoxelShape {
        self.shapes
            .get(id.0 as usize)
            .copied()
            .unwrap_or(VoxelShape::EMPTY)
    }

    /// Returns the number of registered shapes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.shapes.len()
    }

    /// Returns true if no shapes are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty()
    }

    /// Freezes the registry, preventing further registrations.
    pub fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

const FULL_BLOCK_BOXES: &[BlockLocalAabb] = &[BlockLocalAabb::FULL_BLOCK];

/// Shape data for a block state.
#[derive(Debug, Clone, Copy)]
pub struct BlockShapes {
    pub collision: VoxelShape,
    pub support: VoxelShape,
    pub outline: VoxelShape,
    pub occlusion: VoxelShape,
    pub interaction: VoxelShape,
    pub visual: VoxelShape,
}

impl BlockShapes {
    /// Creates new block shapes.
    #[must_use]
    pub const fn new(
        collision: VoxelShape,
        support: VoxelShape,
        outline: VoxelShape,
        occlusion: VoxelShape,
        interaction: VoxelShape,
        visual: VoxelShape,
    ) -> Self {
        Self {
            collision,
            support,
            outline,
            occlusion,
            interaction,
            visual,
        }
    }

    /// Full block for every shape channel except interaction.
    pub const FULL_BLOCK: BlockShapes = BlockShapes::new(
        VoxelShape::FULL_BLOCK,
        VoxelShape::FULL_BLOCK,
        VoxelShape::FULL_BLOCK,
        VoxelShape::FULL_BLOCK,
        VoxelShape::EMPTY,
        VoxelShape::FULL_BLOCK,
    );

    /// Empty shapes for all shape channels.
    pub const EMPTY: BlockShapes = BlockShapes::new(
        VoxelShape::EMPTY,
        VoxelShape::EMPTY,
        VoxelShape::EMPTY,
        VoxelShape::EMPTY,
        VoxelShape::EMPTY,
        VoxelShape::EMPTY,
    );
}

use super::properties::Direction;

/// Returns the overall bounding box of a voxel shape (union of all AABBs).
///
/// The shape must be non-empty; panics otherwise.
#[must_use]
pub fn bounding_box(shape: VoxelShape) -> BlockLocalAabb {
    match shape.bounds() {
        Some(bounds) => bounds,
        None => panic!("bounding_box called on empty shape"),
    }
}

/// Checks if a shape is a full block (covers the entire 0-1 cube).
///
/// This matches vanilla's `Block.isShapeFullBlock()` used by `isSolidRender()`.
///
/// TODO: Handle multi-AABB shapes whose union covers the full block (e.g. stacked slabs).
/// Vanilla uses exact boolean voxel arithmetic (`Shapes.joinIsNotEmpty`). No vanilla blocks
/// currently have multi-AABB full-block shapes, so single-AABB fast path suffices for now.
#[must_use]
pub fn is_shape_full_block(shape: VoxelShape) -> bool {
    // A full block shape must have exactly one box that covers 0-1 on all axes.
    let [aabb] = shape.boxes() else {
        return false;
    };

    aabb.min_x() <= 0.0
        && aabb.max_x() >= 1.0
        && aabb.min_y() <= 0.0
        && aabb.max_y() >= 1.0
        && aabb.min_z() <= 0.0
        && aabb.max_z() >= 1.0
}

/// Support type for `is_face_sturdy` checks.
///
/// Determines what kind of support a block face provides for other blocks.
/// Used by fences, walls, torches, etc. to decide if they can connect/attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportType {
    /// Full face support - the entire face must be solid.
    /// Used by most blocks that need a solid surface.
    Full,
    /// Center support - only the center of the face needs to be solid.
    /// Used by things like hanging signs that only need a small attachment point.
    Center,
    /// Rigid support - most of the face must be solid, but allows small gaps.
    /// Used by bells and similar blocks.
    Rigid,
}

/// Vanilla `SupportType.CENTER`: `Block.column(2.0, 0.0, 10.0)`.
const CENTER_SUPPORT_MIN: f64 = 7.0 / 16.0;
const CENTER_SUPPORT_MAX: f64 = 9.0 / 16.0;
const CENTER_SUPPORT_Y_MAX: f64 = 10.0 / 16.0;

/// Rigid support requires coverage except for a 2-pixel border.
const RIGID_BORDER: f64 = 0.125; // 2/16

/// Checks if a shape fully covers a face (for `SupportType::Full`).
///
/// Returns true if the 2D projection of the shape on the given face
/// completely covers the 1x1 face area.
#[must_use]
pub fn is_face_full(shape: VoxelShape, direction: Direction) -> bool {
    face_rectangles_cover(shape, direction, 0.0, 1.0, 0.0, 1.0)
}

/// Checks if a shape provides center support on a face.
///
/// The center area is a 12x12 pixel region (0.125 to 0.875 on each axis).
#[must_use]
pub fn is_face_center_supported(shape: VoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    match direction {
        Direction::Down | Direction::Up => face_rectangles_cover(
            shape,
            direction,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
        ),
        Direction::North | Direction::South => face_rectangles_cover(
            shape,
            direction,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
            0.0,
            CENTER_SUPPORT_Y_MAX,
        ),
        Direction::West | Direction::East => face_rectangles_cover(
            shape,
            direction,
            0.0,
            CENTER_SUPPORT_Y_MAX,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
        ),
    }
}

/// Checks if a shape provides rigid support on a face.
///
/// Rigid support requires coverage of most of the face except a small border.
#[must_use]
pub fn is_face_rigid_supported(shape: VoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    // For rigid support, we need the shape to cover from RIGID_BORDER to 1-RIGID_BORDER
    let min_bound = RIGID_BORDER;
    let max_bound = 1.0 - RIGID_BORDER;

    match direction {
        Direction::Down => shape.iter().any(|aabb| {
            aabb.min_y() <= 0.0
                && aabb.min_x() <= min_bound
                && aabb.max_x() >= max_bound
                && aabb.min_z() <= min_bound
                && aabb.max_z() >= max_bound
        }),
        Direction::Up => shape.iter().any(|aabb| {
            aabb.max_y() >= 1.0
                && aabb.min_x() <= min_bound
                && aabb.max_x() >= max_bound
                && aabb.min_z() <= min_bound
                && aabb.max_z() >= max_bound
        }),
        Direction::North => shape.iter().any(|aabb| {
            aabb.min_z() <= 0.0
                && aabb.min_x() <= min_bound
                && aabb.max_x() >= max_bound
                && aabb.min_y() <= min_bound
                && aabb.max_y() >= max_bound
        }),
        Direction::South => shape.iter().any(|aabb| {
            aabb.max_z() >= 1.0
                && aabb.min_x() <= min_bound
                && aabb.max_x() >= max_bound
                && aabb.min_y() <= min_bound
                && aabb.max_y() >= max_bound
        }),
        Direction::West => shape.iter().any(|aabb| {
            aabb.min_x() <= 0.0
                && aabb.min_y() <= min_bound
                && aabb.max_y() >= max_bound
                && aabb.min_z() <= min_bound
                && aabb.max_z() >= max_bound
        }),
        Direction::East => shape.iter().any(|aabb| {
            aabb.max_x() >= 1.0
                && aabb.min_y() <= min_bound
                && aabb.max_y() >= max_bound
                && aabb.min_z() <= min_bound
                && aabb.max_z() >= max_bound
        }),
    }
}

/// Checks if a shape is sturdy on a face for the given support type.
#[must_use]
pub fn is_face_sturdy(shape: VoxelShape, direction: Direction, support_type: SupportType) -> bool {
    match support_type {
        SupportType::Full => is_face_full(shape, direction),
        SupportType::Center => is_face_center_supported(shape, direction),
        SupportType::Rigid => is_face_rigid_supported(shape, direction),
    }
}

#[derive(Clone, Copy)]
struct FaceRect {
    min_a: f64,
    max_a: f64,
    min_b: f64,
    max_b: f64,
}

const FACE_EPSILON: f64 = 1.0e-6;

fn face_rectangles_cover(
    shape: VoxelShape,
    direction: Direction,
    target_min_a: f64,
    target_max_a: f64,
    target_min_b: f64,
    target_max_b: f64,
) -> bool {
    let mut rects = Vec::new();
    for aabb in shape {
        let Some(rect) = face_rect_for_aabb(*aabb, direction) else {
            continue;
        };
        if rect.max_a <= target_min_a
            || rect.min_a >= target_max_a
            || rect.max_b <= target_min_b
            || rect.min_b >= target_max_b
        {
            continue;
        }
        rects.push(FaceRect {
            min_a: rect.min_a.max(target_min_a),
            max_a: rect.max_a.min(target_max_a),
            min_b: rect.min_b.max(target_min_b),
            max_b: rect.max_b.min(target_max_b),
        });
    }

    if rects.is_empty() {
        return false;
    }

    let mut a_edges = vec![target_min_a, target_max_a];
    let mut b_edges = vec![target_min_b, target_max_b];
    for rect in &rects {
        a_edges.push(rect.min_a);
        a_edges.push(rect.max_a);
        b_edges.push(rect.min_b);
        b_edges.push(rect.max_b);
    }
    sort_and_dedup_edges(&mut a_edges);
    sort_and_dedup_edges(&mut b_edges);

    for a_pair in a_edges.windows(2) {
        if a_pair[1] - a_pair[0] <= FACE_EPSILON {
            continue;
        }
        for b_pair in b_edges.windows(2) {
            if b_pair[1] - b_pair[0] <= FACE_EPSILON {
                continue;
            }
            let covered = rects.iter().any(|rect| {
                rect.min_a <= a_pair[0] + FACE_EPSILON
                    && rect.max_a >= a_pair[1] - FACE_EPSILON
                    && rect.min_b <= b_pair[0] + FACE_EPSILON
                    && rect.max_b >= b_pair[1] - FACE_EPSILON
            });
            if !covered {
                return false;
            }
        }
    }

    true
}

fn face_rect_for_aabb(aabb: BlockLocalAabb, direction: Direction) -> Option<FaceRect> {
    let rect = match direction {
        Direction::Down if aabb.min_y() <= FACE_EPSILON => FaceRect {
            min_a: aabb.min_x(),
            max_a: aabb.max_x(),
            min_b: aabb.min_z(),
            max_b: aabb.max_z(),
        },
        Direction::Up if aabb.max_y() >= 1.0 - FACE_EPSILON => FaceRect {
            min_a: aabb.min_x(),
            max_a: aabb.max_x(),
            min_b: aabb.min_z(),
            max_b: aabb.max_z(),
        },
        Direction::North if aabb.min_z() <= FACE_EPSILON => FaceRect {
            min_a: aabb.min_x(),
            max_a: aabb.max_x(),
            min_b: aabb.min_y(),
            max_b: aabb.max_y(),
        },
        Direction::South if aabb.max_z() >= 1.0 - FACE_EPSILON => FaceRect {
            min_a: aabb.min_x(),
            max_a: aabb.max_x(),
            min_b: aabb.min_y(),
            max_b: aabb.max_y(),
        },
        Direction::West if aabb.min_x() <= FACE_EPSILON => FaceRect {
            min_a: aabb.min_y(),
            max_a: aabb.max_y(),
            min_b: aabb.min_z(),
            max_b: aabb.max_z(),
        },
        Direction::East if aabb.max_x() >= 1.0 - FACE_EPSILON => FaceRect {
            min_a: aabb.min_y(),
            max_a: aabb.max_y(),
            min_b: aabb.min_z(),
            max_b: aabb.max_z(),
        },
        _ => return None,
    };

    if rect.min_a >= rect.max_a || rect.min_b >= rect.max_b {
        return None;
    }
    Some(rect)
}

fn sort_and_dedup_edges(edges: &mut Vec<f64>) {
    edges.sort_by(|a, b| a.total_cmp(b));
    edges.dedup_by(|a, b| (*a - *b).abs() <= FACE_EPSILON);
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUADRANT_TOP_FACE: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.5, 0.0, 0.5, 1.0, 0.5),
        BlockLocalAabb::new(0.5, 0.5, 0.0, 1.0, 1.0, 0.5),
        BlockLocalAabb::new(0.0, 0.5, 0.5, 0.5, 1.0, 1.0),
        BlockLocalAabb::new(0.5, 0.5, 0.5, 1.0, 1.0, 1.0),
    ];

    const GAPPED_TOP_FACE: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.5, 0.0, 0.45, 1.0, 1.0),
        BlockLocalAabb::new(0.55, 0.5, 0.0, 1.0, 1.0, 1.0),
    ];

    const VANILLA_AZALEA_SHAPE: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.375, 0.0, 0.375, 0.625, 1.0, 0.625),
        BlockLocalAabb::new(0.0, 0.5, 0.0, 0.375, 1.0, 1.0),
        BlockLocalAabb::new(0.375, 0.5, 0.0, 1.0, 1.0, 0.375),
        BlockLocalAabb::new(0.375, 0.5, 0.625, 1.0, 1.0, 1.0),
        BlockLocalAabb::new(0.625, 0.5, 0.375, 1.0, 1.0, 0.625),
    ];

    #[test]
    fn face_full_accepts_union_covering_face() {
        assert!(is_face_full(
            VoxelShape::from_boxes(QUADRANT_TOP_FACE),
            Direction::Up
        ));
    }

    #[test]
    fn face_full_rejects_union_with_gap() {
        assert!(!is_face_full(
            VoxelShape::from_boxes(GAPPED_TOP_FACE),
            Direction::Up
        ));
    }

    #[test]
    fn face_full_accepts_vanilla_azalea_top_shape() {
        assert!(is_face_full(
            VoxelShape::from_boxes(VANILLA_AZALEA_SHAPE),
            Direction::Up
        ));
    }
}
