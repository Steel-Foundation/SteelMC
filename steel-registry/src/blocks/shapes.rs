use glam::DVec3;
use steel_utils::{BlockLocalAabb, axis::Axis};

/// Vanilla shape boolean operation.
///
/// Mirrors `net.minecraft.world.phys.shapes.BooleanOp`. Operations where
/// `apply(false, false)` is true are not valid for `join_is_not_empty`, matching
/// vanilla's guard for unbounded outside-space results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    False,
    NotOr,
    OnlySecond,
    NotFirst,
    OnlyFirst,
    NotSecond,
    NotSame,
    NotAnd,
    And,
    Same,
    Second,
    Causes,
    First,
    CausedBy,
    Or,
    True,
}

impl BooleanOp {
    #[must_use]
    pub const fn apply(self, first: bool, second: bool) -> bool {
        match self {
            Self::False => false,
            Self::NotOr => !first && !second,
            Self::OnlySecond => second && !first,
            Self::NotFirst => !first,
            Self::OnlyFirst => first && !second,
            Self::NotSecond => !second,
            Self::NotSame => first != second,
            Self::NotAnd => !first || !second,
            Self::And => first && second,
            Self::Same => first == second,
            Self::Second => second,
            Self::Causes => !first || second,
            Self::First => first,
            Self::CausedBy => first || !second,
            Self::Or => first || second,
            Self::True => true,
        }
    }
}

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

    /// Returns true if this shape has no non-empty boxes.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.boxes.iter().all(|aabb| aabb.is_empty())
    }

    /// Returns the minimum coordinate on `axis`, or positive infinity for an empty shape.
    #[must_use]
    pub fn min(self, axis: Axis) -> f64 {
        self.boxes
            .iter()
            .filter(|aabb| !aabb.is_empty())
            .map(|aabb| aabb.min(axis))
            .fold(f64::INFINITY, f64::min)
    }

    /// Returns the maximum coordinate on `axis`, or negative infinity for an empty shape.
    #[must_use]
    pub fn max(self, axis: Axis) -> f64 {
        self.boxes
            .iter()
            .filter(|aabb| !aabb.is_empty())
            .map(|aabb| aabb.max(axis))
            .fold(f64::NEG_INFINITY, f64::max)
    }

    /// Returns the union bounds of this shape, or `None` for empty shapes.
    #[must_use]
    pub fn bounds(self) -> Option<BlockLocalAabb> {
        let first = self.boxes.iter().find(|aabb| !aabb.is_empty())?;
        let mut min_x = first.min_x();
        let mut min_y = first.min_y();
        let mut min_z = first.min_z();
        let mut max_x = first.max_x();
        let mut max_y = first.max_y();
        let mut max_z = first.max_z();

        for aabb in self.boxes {
            if aabb.is_empty() {
                continue;
            }
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

    /// Returns true when this shape extends outside its owning block.
    ///
    /// Mirrors vanilla `BlockState.hasLargeCollisionShape()` for collision
    /// iterator filtering.
    #[must_use]
    pub fn has_large_collision_shape(self) -> bool {
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .any(|axis| self.min(axis) < 0.0 || self.max(axis) > 1.0)
    }
}

impl IntoIterator for VoxelShape {
    type IntoIter = core::slice::Iter<'static, BlockLocalAabb>;
    type Item = &'static BlockLocalAabb;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A voxel shape plus the block-local offset from vanilla `BlockState.getOffset(pos)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetVoxelShape {
    shape: VoxelShape,
    offset: DVec3,
}

impl OffsetVoxelShape {
    #[must_use]
    pub const fn new(shape: VoxelShape, offset: DVec3) -> Self {
        Self { shape, offset }
    }

    #[must_use]
    pub const fn without_offset(shape: VoxelShape) -> Self {
        Self {
            shape,
            offset: DVec3::ZERO,
        }
    }

    #[must_use]
    pub const fn shape(self) -> VoxelShape {
        self.shape
    }

    #[must_use]
    pub const fn offset(self) -> DVec3 {
        self.offset
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        self.shape.is_empty()
    }

    pub fn iter(self) -> impl Iterator<Item = BlockLocalAabb> {
        self.shape
            .into_iter()
            .map(move |aabb| aabb.translate(self.offset))
    }

    #[must_use]
    pub fn min(self, axis: Axis) -> f64 {
        self.shape.min(axis) + axis_offset(self.offset, axis)
    }

    #[must_use]
    pub fn max(self, axis: Axis) -> f64 {
        self.shape.max(axis) + axis_offset(self.offset, axis)
    }

    #[must_use]
    pub fn bounds(self) -> Option<BlockLocalAabb> {
        self.shape
            .bounds()
            .map(|bounds| bounds.translate(self.offset))
    }

    #[must_use]
    pub fn has_large_collision_shape(self) -> bool {
        [Axis::X, Axis::Y, Axis::Z]
            .into_iter()
            .any(|axis| self.min(axis) < 0.0 || self.max(axis) > 1.0)
    }
}

fn axis_offset(offset: DVec3, axis: Axis) -> f64 {
    match axis {
        Axis::X => offset.x,
        Axis::Y => offset.y,
        Axis::Z => offset.z,
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

const VOXEL_EPSILON: f64 = 1.0e-7;

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

/// Shape channel names used by vanilla block-state shape queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeChannel {
    Collision,
    Support,
    Outline,
    Occlusion,
    Interaction,
    Visual,
}

/// Records which extracted shape channels already include `BlockState.getOffset(pos)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeOffsetFlags {
    collision: bool,
    support: bool,
    outline: bool,
    occlusion: bool,
    interaction: bool,
    visual: bool,
}

impl ShapeOffsetFlags {
    pub const NONE: Self = Self::new(false, false, false, false, false, false);

    #[must_use]
    pub const fn new(
        collision: bool,
        support: bool,
        outline: bool,
        occlusion: bool,
        interaction: bool,
        visual: bool,
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

    #[must_use]
    pub const fn uses_offset(self, channel: ShapeChannel) -> bool {
        match channel {
            ShapeChannel::Collision => self.collision,
            ShapeChannel::Support => self.support,
            ShapeChannel::Outline => self.outline,
            ShapeChannel::Occlusion => self.occlusion,
            ShapeChannel::Interaction => self.interaction,
            ShapeChannel::Visual => self.visual,
        }
    }
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
#[must_use]
pub fn is_shape_full_block(shape: VoxelShape) -> bool {
    !join_is_not_empty(VoxelShape::FULL_BLOCK, shape, BooleanOp::NotSame)
}

#[must_use]
pub fn is_offset_shape_full_block(shape: OffsetVoxelShape) -> bool {
    if shape.offset == DVec3::ZERO {
        return is_shape_full_block(shape.shape);
    }

    if shape.is_empty()
        || shape.min(Axis::X) > VOXEL_EPSILON
        || shape.max(Axis::X) < 1.0 - VOXEL_EPSILON
        || shape.min(Axis::Y) > VOXEL_EPSILON
        || shape.max(Axis::Y) < 1.0 - VOXEL_EPSILON
        || shape.min(Axis::Z) > VOXEL_EPSILON
        || shape.max(Axis::Z) < 1.0 - VOXEL_EPSILON
    {
        return false;
    }

    let mut x_edges = vec![0.0, 1.0];
    let mut y_edges = vec![0.0, 1.0];
    let mut z_edges = vec![0.0, 1.0];
    for aabb in shape.iter() {
        if aabb.is_empty() {
            continue;
        }
        if aabb.max_x() > VOXEL_EPSILON && aabb.min_x() < 1.0 - VOXEL_EPSILON {
            x_edges.push(aabb.min_x().clamp(0.0, 1.0));
            x_edges.push(aabb.max_x().clamp(0.0, 1.0));
        }
        if aabb.max_y() > VOXEL_EPSILON && aabb.min_y() < 1.0 - VOXEL_EPSILON {
            y_edges.push(aabb.min_y().clamp(0.0, 1.0));
            y_edges.push(aabb.max_y().clamp(0.0, 1.0));
        }
        if aabb.max_z() > VOXEL_EPSILON && aabb.min_z() < 1.0 - VOXEL_EPSILON {
            z_edges.push(aabb.min_z().clamp(0.0, 1.0));
            z_edges.push(aabb.max_z().clamp(0.0, 1.0));
        }
    }
    sort_and_dedup_voxel_edges(&mut x_edges);
    sort_and_dedup_voxel_edges(&mut y_edges);
    sort_and_dedup_voxel_edges(&mut z_edges);

    for x in x_edges.windows(2) {
        if x[1] - x[0] <= VOXEL_EPSILON {
            continue;
        }
        for y in y_edges.windows(2) {
            if y[1] - y[0] <= VOXEL_EPSILON {
                continue;
            }
            for z in z_edges.windows(2) {
                if z[1] - z[0] <= VOXEL_EPSILON {
                    continue;
                }
                if !offset_shape_fills_cell(shape, x[0], x[1], y[0], y[1], z[0], z[1]) {
                    return false;
                }
            }
        }
    }

    true
}

/// Returns true if applying `op` to two voxel shapes produces any filled space.
///
/// This is the box-backed equivalent of vanilla `Shapes.joinIsNotEmpty`. It
/// decomposes both shapes into a shared coordinate grid and tests occupancy in
/// each cell. The current representation does not materialize a joined shape;
/// it answers the boolean query needed for full-block and occlusion checks.
///
/// # Panics
/// Panics if `op.apply(false, false)` is true, matching vanilla's invalid
/// operation guard for unbounded outside-space results.
#[must_use]
pub fn join_is_not_empty(first: VoxelShape, second: VoxelShape, op: BooleanOp) -> bool {
    if op.apply(false, false) {
        panic!("join_is_not_empty cannot use an operation that includes empty outside space");
    }

    let first_empty = first.is_empty();
    let second_empty = second.is_empty();
    if first_empty || second_empty {
        return op.apply(!first_empty, !second_empty);
    }

    if first == second {
        return op.apply(true, true);
    }

    let first_only_matters = op.apply(true, false);
    let second_only_matters = op.apply(false, true);
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        if first.max(axis) < second.min(axis) - VOXEL_EPSILON {
            return first_only_matters || second_only_matters;
        }
        if second.max(axis) < first.min(axis) - VOXEL_EPSILON {
            return first_only_matters || second_only_matters;
        }
    }

    let mut x_edges = shape_edges(first, second, Axis::X);
    let mut y_edges = shape_edges(first, second, Axis::Y);
    let mut z_edges = shape_edges(first, second, Axis::Z);
    sort_and_dedup_voxel_edges(&mut x_edges);
    sort_and_dedup_voxel_edges(&mut y_edges);
    sort_and_dedup_voxel_edges(&mut z_edges);

    for x in x_edges.windows(2) {
        if x[1] - x[0] <= VOXEL_EPSILON {
            continue;
        }
        for y in y_edges.windows(2) {
            if y[1] - y[0] <= VOXEL_EPSILON {
                continue;
            }
            for z in z_edges.windows(2) {
                if z[1] - z[0] <= VOXEL_EPSILON {
                    continue;
                }
                let first_full = shape_fills_cell(first, x[0], x[1], y[0], y[1], z[0], z[1]);
                let second_full = shape_fills_cell(second, x[0], x[1], y[0], y[1], z[0], z[1]);
                if op.apply(first_full, second_full) {
                    return true;
                }
            }
        }
    }

    false
}

/// Materializes the unoptimized cell boxes produced by a shape boolean operation.
///
/// Vanilla parity: `Shapes.joinUnoptimized(first, second, op)`, expressed as
/// block-local boxes instead of a lazily merged voxel shape.
///
/// # Panics
/// Panics if `op.apply(false, false)` is true, matching vanilla's invalid
/// operation guard for unbounded outside-space results.
#[must_use]
pub fn join_unoptimized_boxes(
    first: VoxelShape,
    second: VoxelShape,
    op: BooleanOp,
) -> Vec<BlockLocalAabb> {
    if op.apply(false, false) {
        panic!("join_unoptimized_boxes cannot use an operation that includes empty outside space");
    }

    if first.is_empty() && second.is_empty() {
        return Vec::new();
    }

    let mut x_edges = shape_edges(first, second, Axis::X);
    let mut y_edges = shape_edges(first, second, Axis::Y);
    let mut z_edges = shape_edges(first, second, Axis::Z);
    sort_and_dedup_voxel_edges(&mut x_edges);
    sort_and_dedup_voxel_edges(&mut y_edges);
    sort_and_dedup_voxel_edges(&mut z_edges);

    let mut boxes = Vec::new();
    for x in x_edges.windows(2) {
        if x[1] - x[0] <= VOXEL_EPSILON {
            continue;
        }
        for y in y_edges.windows(2) {
            if y[1] - y[0] <= VOXEL_EPSILON {
                continue;
            }
            for z in z_edges.windows(2) {
                if z[1] - z[0] <= VOXEL_EPSILON {
                    continue;
                }

                let first_full = shape_fills_cell(first, x[0], x[1], y[0], y[1], z[0], z[1]);
                let second_full = shape_fills_cell(second, x[0], x[1], y[0], y[1], z[0], z[1]);
                if op.apply(first_full, second_full) {
                    boxes.push(BlockLocalAabb::new(x[0], y[0], z[0], x[1], y[1], z[1]));
                }
            }
        }
    }

    boxes
}

fn shape_edges(first: VoxelShape, second: VoxelShape, axis: Axis) -> Vec<f64> {
    let mut edges = Vec::with_capacity((first.len() + second.len()) * 2);
    for shape in [first, second] {
        for aabb in shape {
            if aabb.is_empty() {
                continue;
            }
            edges.push(aabb.min(axis));
            edges.push(aabb.max(axis));
        }
    }
    edges
}

fn sort_and_dedup_voxel_edges(edges: &mut Vec<f64>) {
    edges.sort_by(|a, b| a.total_cmp(b));
    edges.dedup_by(|a, b| (*a - *b).abs() <= VOXEL_EPSILON);
}

fn shape_fills_cell(
    shape: VoxelShape,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
) -> bool {
    shape.into_iter().any(|aabb| {
        !aabb.is_empty()
            && aabb.min_x() <= min_x + VOXEL_EPSILON
            && aabb.max_x() >= max_x - VOXEL_EPSILON
            && aabb.min_y() <= min_y + VOXEL_EPSILON
            && aabb.max_y() >= max_y - VOXEL_EPSILON
            && aabb.min_z() <= min_z + VOXEL_EPSILON
            && aabb.max_z() >= max_z - VOXEL_EPSILON
    })
}

fn offset_shape_fills_cell(
    shape: OffsetVoxelShape,
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    min_z: f64,
    max_z: f64,
) -> bool {
    shape.iter().any(|aabb| {
        !aabb.is_empty()
            && aabb.min_x() <= min_x + VOXEL_EPSILON
            && aabb.max_x() >= max_x - VOXEL_EPSILON
            && aabb.min_y() <= min_y + VOXEL_EPSILON
            && aabb.max_y() >= max_y - VOXEL_EPSILON
            && aabb.min_z() <= min_z + VOXEL_EPSILON
            && aabb.max_z() >= max_z - VOXEL_EPSILON
    })
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

/// Vanilla `SupportType.RIGID`: `Shapes.block() ONLY_FIRST Block.column(12.0, 0.0, 16.0)`.
const RIGID_BORDER: f64 = 0.125; // 2/16

/// Checks if a shape fully covers a face (for `SupportType::Full`).
///
/// Returns true if the 2D projection of the shape on the given face
/// completely covers the 1x1 face area.
#[must_use]
pub fn is_face_full(shape: VoxelShape, direction: Direction) -> bool {
    face_rectangles_cover(shape, direction, 0.0, 1.0, 0.0, 1.0)
}

#[must_use]
pub fn is_offset_face_full(shape: OffsetVoxelShape, direction: Direction) -> bool {
    offset_face_rectangles_cover(shape, direction, 0.0, 1.0, 0.0, 1.0)
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

#[must_use]
pub fn is_offset_face_center_supported(shape: OffsetVoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    match direction {
        Direction::Down | Direction::Up => offset_face_rectangles_cover(
            shape,
            direction,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
        ),
        Direction::North | Direction::South => offset_face_rectangles_cover(
            shape,
            direction,
            CENTER_SUPPORT_MIN,
            CENTER_SUPPORT_MAX,
            0.0,
            CENTER_SUPPORT_Y_MAX,
        ),
        Direction::West | Direction::East => offset_face_rectangles_cover(
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
/// Rigid support requires coverage of vanilla's fixed 3D support mask.
#[must_use]
pub fn is_face_rigid_supported(shape: VoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    match direction {
        Direction::Down | Direction::Up => {
            face_rectangles_cover(shape, direction, 0.0, RIGID_BORDER, 0.0, 1.0)
                && face_rectangles_cover(shape, direction, 1.0 - RIGID_BORDER, 1.0, 0.0, 1.0)
                && face_rectangles_cover(
                    shape,
                    direction,
                    RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    0.0,
                    RIGID_BORDER,
                )
                && face_rectangles_cover(
                    shape,
                    direction,
                    RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    1.0,
                )
        }
        Direction::North | Direction::South | Direction::West | Direction::East => {
            is_face_full(shape, direction)
        }
    }
}

#[must_use]
pub fn is_offset_face_rigid_supported(shape: OffsetVoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    match direction {
        Direction::Down | Direction::Up => {
            offset_face_rectangles_cover(shape, direction, 0.0, RIGID_BORDER, 0.0, 1.0)
                && offset_face_rectangles_cover(shape, direction, 1.0 - RIGID_BORDER, 1.0, 0.0, 1.0)
                && offset_face_rectangles_cover(
                    shape,
                    direction,
                    RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    0.0,
                    RIGID_BORDER,
                )
                && offset_face_rectangles_cover(
                    shape,
                    direction,
                    RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    1.0 - RIGID_BORDER,
                    1.0,
                )
        }
        Direction::North | Direction::South | Direction::West | Direction::East => {
            is_offset_face_full(shape, direction)
        }
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

#[must_use]
pub fn is_offset_face_sturdy(
    shape: OffsetVoxelShape,
    direction: Direction,
    support_type: SupportType,
) -> bool {
    match support_type {
        SupportType::Full => is_offset_face_full(shape, direction),
        SupportType::Center => is_offset_face_center_supported(shape, direction),
        SupportType::Rigid => is_offset_face_rigid_supported(shape, direction),
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

pub fn face_rectangles_cover(
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

    face_rects_cover_target(
        rects,
        target_min_a,
        target_max_a,
        target_min_b,
        target_max_b,
    )
}

pub fn offset_face_rectangles_cover(
    shape: OffsetVoxelShape,
    direction: Direction,
    target_min_a: f64,
    target_max_a: f64,
    target_min_b: f64,
    target_max_b: f64,
) -> bool {
    let mut rects = Vec::new();
    for aabb in shape.iter() {
        let Some(rect) = face_rect_for_aabb(aabb, direction) else {
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

    face_rects_cover_target(
        rects,
        target_min_a,
        target_max_a,
        target_min_b,
        target_max_b,
    )
}

fn face_rects_cover_target(
    rects: Vec<FaceRect>,
    target_min_a: f64,
    target_max_a: f64,
    target_min_b: f64,
    target_max_b: f64,
) -> bool {
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

    const SPLIT_FULL_BLOCK: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.0, 0.0, 0.5, 1.0, 1.0),
        BlockLocalAabb::new(0.5, 0.0, 0.0, 1.0, 1.0, 1.0),
    ];

    const Z_GAPPED_BLOCK_WITH_OFFSET: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.0, -0.1, 1.0, 1.0, 0.15),
        BlockLocalAabb::new(0.0, 0.0, 0.65, 1.0, 1.0, 0.9),
    ];

    const LOWER_HALF_BLOCK: &[BlockLocalAabb] =
        &[BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.5, 1.0)];

    const UPPER_HALF_BLOCK: &[BlockLocalAabb] =
        &[BlockLocalAabb::new(0.0, 0.5, 0.0, 1.0, 1.0, 1.0)];

    const OVERLAPPING_HALF_BLOCKS: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.0, 0.0, 0.75, 1.0, 1.0),
        BlockLocalAabb::new(0.25, 0.0, 0.0, 1.0, 1.0, 1.0),
    ];

    const ZERO_VOLUME_BOX: &[BlockLocalAabb] = &[BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, 0.0, 1.0)];

    const LARGE_COLLISION_SHAPE: &[BlockLocalAabb] =
        &[BlockLocalAabb::new(-0.25, 0.0, 0.0, 1.0, 1.0, 1.0)];

    const RIGID_TOP_RING: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.0, 0.0, RIGID_BORDER, 1.0, 1.0),
        BlockLocalAabb::new(1.0 - RIGID_BORDER, 0.0, 0.0, 1.0, 1.0, 1.0),
        BlockLocalAabb::new(
            RIGID_BORDER,
            0.0,
            0.0,
            1.0 - RIGID_BORDER,
            1.0,
            RIGID_BORDER,
        ),
        BlockLocalAabb::new(
            RIGID_BORDER,
            0.0,
            1.0 - RIGID_BORDER,
            1.0 - RIGID_BORDER,
            1.0,
            1.0,
        ),
    ];

    const RIGID_CENTER_PANEL: &[BlockLocalAabb] = &[BlockLocalAabb::new(
        RIGID_BORDER,
        0.0,
        RIGID_BORDER,
        1.0 - RIGID_BORDER,
        1.0,
        1.0 - RIGID_BORDER,
    )];

    const RIGID_WEST_FACE_RING: &[BlockLocalAabb] = &[
        BlockLocalAabb::new(0.0, 0.0, 0.0, 1.0, RIGID_BORDER, 1.0),
        BlockLocalAabb::new(0.0, 1.0 - RIGID_BORDER, 0.0, 1.0, 1.0, 1.0),
        BlockLocalAabb::new(
            0.0,
            RIGID_BORDER,
            0.0,
            1.0,
            1.0 - RIGID_BORDER,
            RIGID_BORDER,
        ),
        BlockLocalAabb::new(
            0.0,
            RIGID_BORDER,
            1.0 - RIGID_BORDER,
            1.0,
            1.0 - RIGID_BORDER,
            1.0,
        ),
    ];

    #[test]
    fn boolean_op_matches_vanilla_truth_table() {
        assert!(BooleanOp::OnlyFirst.apply(true, false));
        assert!(!BooleanOp::OnlyFirst.apply(false, true));
        assert!(BooleanOp::NotSame.apply(true, false));
        assert!(!BooleanOp::NotSame.apply(true, true));
        assert!(BooleanOp::Or.apply(false, true));
        assert!(!BooleanOp::And.apply(true, false));
    }

    #[test]
    fn join_is_not_empty_detects_intersection() {
        assert!(join_is_not_empty(
            VoxelShape::from_boxes(OVERLAPPING_HALF_BLOCKS),
            VoxelShape::from_boxes(LOWER_HALF_BLOCK),
            BooleanOp::And
        ));
    }

    #[test]
    fn join_is_not_empty_rejects_disjoint_and() {
        assert!(!join_is_not_empty(
            VoxelShape::from_boxes(LOWER_HALF_BLOCK),
            VoxelShape::from_boxes(UPPER_HALF_BLOCK),
            BooleanOp::And
        ));
    }

    #[test]
    fn join_is_not_empty_detects_only_first_remainder() {
        assert!(join_is_not_empty(
            VoxelShape::FULL_BLOCK,
            VoxelShape::from_boxes(LOWER_HALF_BLOCK),
            BooleanOp::OnlyFirst
        ));
    }

    #[test]
    fn join_unoptimized_boxes_materializes_only_second_remainder() {
        let remainder = join_unoptimized_boxes(
            VoxelShape::from_boxes(LOWER_HALF_BLOCK),
            VoxelShape::FULL_BLOCK,
            BooleanOp::OnlySecond,
        );

        assert_eq!(
            remainder,
            vec![BlockLocalAabb::new(0.0, 0.5, 0.0, 1.0, 1.0, 1.0)]
        );
    }

    #[test]
    fn shape_full_block_accepts_tiled_boxes() {
        assert!(is_shape_full_block(VoxelShape::from_boxes(
            SPLIT_FULL_BLOCK
        )));
    }

    #[test]
    fn shape_full_block_rejects_partial_boxes() {
        assert!(!is_shape_full_block(VoxelShape::from_boxes(
            LOWER_HALF_BLOCK
        )));
    }

    #[test]
    fn offset_shape_full_block_rejects_shifted_full_block() {
        assert!(is_offset_shape_full_block(
            OffsetVoxelShape::without_offset(VoxelShape::FULL_BLOCK)
        ));
        assert!(!is_offset_shape_full_block(OffsetVoxelShape::new(
            VoxelShape::FULL_BLOCK,
            DVec3::new(0.25, 0.0, 0.0)
        )));
    }

    #[test]
    fn offset_shape_full_block_rejects_z_gap_after_offset() {
        assert!(!is_offset_shape_full_block(OffsetVoxelShape::new(
            VoxelShape::from_boxes(Z_GAPPED_BLOCK_WITH_OFFSET),
            DVec3::new(0.0, 0.0, 0.1)
        )));
    }

    #[test]
    fn zero_volume_boxes_are_empty() {
        assert!(VoxelShape::from_boxes(ZERO_VOLUME_BOX).is_empty());
        assert!(!join_is_not_empty(
            VoxelShape::from_boxes(ZERO_VOLUME_BOX),
            VoxelShape::FULL_BLOCK,
            BooleanOp::And
        ));
    }

    #[test]
    fn large_collision_shape_matches_vanilla_bounds_rule() {
        assert!(!VoxelShape::EMPTY.has_large_collision_shape());
        assert!(!VoxelShape::FULL_BLOCK.has_large_collision_shape());
        assert!(VoxelShape::from_boxes(LARGE_COLLISION_SHAPE).has_large_collision_shape());
    }

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
    fn offset_face_full_rejects_shifted_top_face() {
        assert!(!is_offset_face_full(
            OffsetVoxelShape::new(VoxelShape::FULL_BLOCK, DVec3::new(0.25, 0.0, 0.0)),
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

    #[test]
    fn rigid_support_accepts_border_ring_covered_by_multiple_boxes() {
        assert!(is_face_rigid_supported(
            VoxelShape::from_boxes(RIGID_TOP_RING),
            Direction::Up
        ));
    }

    #[test]
    fn rigid_support_rejects_center_panel_without_border_ring() {
        assert!(!is_face_rigid_supported(
            VoxelShape::from_boxes(RIGID_CENTER_PANEL),
            Direction::Up
        ));
    }

    #[test]
    fn rigid_support_rejects_side_border_ring_without_full_face() {
        assert!(!is_face_rigid_supported(
            VoxelShape::from_boxes(RIGID_WEST_FACE_RING),
            Direction::West
        ));
    }
}
