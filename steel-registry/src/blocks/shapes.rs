/// Axis-Aligned Bounding Box used for block collision and outline shapes.
///
/// Coordinates are in block-local space (0.0 to 1.0 for a standard block).
/// Values can extend beyond 0.0-1.0 for blocks like fences (collision height 1.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABB {
    pub min_x: f32,
    pub min_y: f32,
    pub min_z: f32,
    pub max_x: f32,
    pub max_y: f32,
    pub max_z: f32,
}

impl AABB {
    /// Creates a new AABB from min and max coordinates.
    #[must_use]
    pub const fn new(
        min_x: f32,
        min_y: f32,
        min_z: f32,
        max_x: f32,
        max_y: f32,
        max_z: f32,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// A full block (0,0,0) to (1,1,1).
    pub const FULL_BLOCK: AABB = AABB::new(0.0, 0.0, 0.0, 1.0, 1.0, 1.0);

    /// An empty shape (no collision).
    pub const EMPTY: AABB = AABB::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0);

    /// Returns true if this AABB has no volume.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.min_x >= self.max_x || self.min_y >= self.max_y || self.min_z >= self.max_z
    }

    /// Returns the width (X dimension) of this AABB.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.max_x - self.min_x
    }

    /// Returns the height (Y dimension) of this AABB.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.max_y - self.min_y
    }

    /// Returns the depth (Z dimension) of this AABB.
    #[must_use]
    pub fn depth(&self) -> f32 {
        self.max_z - self.min_z
    }

    /// Returns a new AABB deflated (shrunk inward) by the given amount on all sides.
    ///
    /// This is used for collision detection to avoid floating-point edge cases.
    /// Matches vanilla `AABB.deflate()`.
    #[must_use]
    pub fn deflate(&self, amount: f32) -> Self {
        Self {
            min_x: self.min_x + amount,
            min_y: self.min_y + amount,
            min_z: self.min_z + amount,
            max_x: self.max_x - amount,
            max_y: self.max_y - amount,
            max_z: self.max_z - amount,
        }
    }

    /// Returns a new AABB inflated (expanded outward) by the given amount on all sides.
    ///
    /// Matches vanilla `AABB.inflate()`.
    #[must_use]
    pub fn inflate(&self, amount: f32) -> Self {
        Self {
            min_x: self.min_x - amount,
            min_y: self.min_y - amount,
            min_z: self.min_z - amount,
            max_x: self.max_x + amount,
            max_y: self.max_y + amount,
            max_z: self.max_z + amount,
        }
    }

    /// Returns a new AABB moved by the given delta.
    ///
    /// Matches vanilla `AABB.move()`.
    #[must_use]
    pub fn translate(&self, dx: f32, dy: f32, dz: f32) -> Self {
        Self {
            min_x: self.min_x + dx,
            min_y: self.min_y + dy,
            min_z: self.min_z + dz,
            max_x: self.max_x + dx,
            max_y: self.max_y + dy,
            max_z: self.max_z + dz,
        }
    }

    /// Returns a new AABB positioned at the given block coordinates.
    ///
    /// Converts a block-local AABB (0-1 space) to world coordinates.
    #[must_use]
    pub fn at_block(&self, block_x: i32, block_y: i32, block_z: i32) -> Self {
        let bx = block_x as f32;
        let by = block_y as f32;
        let bz = block_z as f32;
        Self {
            min_x: bx + self.min_x,
            min_y: by + self.min_y,
            min_z: bz + self.min_z,
            max_x: bx + self.max_x,
            max_y: by + self.max_y,
            max_z: bz + self.max_z,
        }
    }

    /// Checks if this AABB intersects with another AABB.
    ///
    /// Returns true if the two AABBs overlap in all three dimensions.
    /// Matches vanilla `AABB.intersects()`.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.max_x > other.min_x
            && self.min_x < other.max_x
            && self.max_y > other.min_y
            && self.min_y < other.max_y
            && self.max_z > other.min_z
            && self.min_z < other.max_z
    }

    /// Checks if this AABB contains the given point.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32, z: f32) -> bool {
        x >= self.min_x
            && x <= self.max_x
            && y >= self.min_y
            && y <= self.max_y
            && z >= self.min_z
            && z <= self.max_z
    }
}

/// Double-precision Axis-Aligned Bounding Box used for entity collision.
///
/// Coordinates are in world space. Used for player and entity bounding boxes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AABBd {
    pub min_x: f64,
    pub min_y: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_y: f64,
    pub max_z: f64,
}

impl AABBd {
    /// Creates a new double-precision AABB from min and max coordinates.
    #[must_use]
    pub const fn new(
        min_x: f64,
        min_y: f64,
        min_z: f64,
        max_x: f64,
        max_y: f64,
        max_z: f64,
    ) -> Self {
        Self {
            min_x,
            min_y,
            min_z,
            max_x,
            max_y,
            max_z,
        }
    }

    /// Creates an entity bounding box centered at the given position.
    ///
    /// The box extends `half_width` in X and Z directions,
    /// and from `y` to `y + height` in the Y direction.
    #[must_use]
    pub fn entity_box(x: f64, y: f64, z: f64, half_width: f64, height: f64) -> Self {
        Self {
            min_x: x - half_width,
            min_y: y,
            min_z: z - half_width,
            max_x: x + half_width,
            max_y: y + height,
            max_z: z + half_width,
        }
    }

    /// Returns a new AABB deflated (shrunk inward) by the given amount on all sides.
    ///
    /// This is used for collision detection to avoid floating-point edge cases.
    /// Matches vanilla's collision epsilon of 1.0E-5.
    #[must_use]
    pub fn deflate(&self, amount: f64) -> Self {
        Self {
            min_x: self.min_x + amount,
            min_y: self.min_y + amount,
            min_z: self.min_z + amount,
            max_x: self.max_x - amount,
            max_y: self.max_y - amount,
            max_z: self.max_z - amount,
        }
    }

    /// Returns a new AABB inflated (expanded outward) by the given amount on all sides.
    #[must_use]
    pub fn inflate(&self, amount: f64) -> Self {
        Self {
            min_x: self.min_x - amount,
            min_y: self.min_y - amount,
            min_z: self.min_z - amount,
            max_x: self.max_x + amount,
            max_y: self.max_y + amount,
            max_z: self.max_z + amount,
        }
    }

    /// Returns a new AABB inflated by different amounts on each axis.
    #[must_use]
    pub fn inflate_xyz(&self, x: f64, y: f64, z: f64) -> Self {
        Self {
            min_x: self.min_x - x,
            min_y: self.min_y - y,
            min_z: self.min_z - z,
            max_x: self.max_x + x,
            max_y: self.max_y + y,
            max_z: self.max_z + z,
        }
    }

    /// Checks if this AABB intersects with another AABB.
    #[must_use]
    pub fn intersects(&self, other: &Self) -> bool {
        self.max_x > other.min_x
            && self.min_x < other.max_x
            && self.max_y > other.min_y
            && self.min_y < other.max_y
            && self.max_z > other.min_z
            && self.min_z < other.max_z
    }

    /// Checks if this AABB intersects with a single-precision block AABB.
    #[must_use]
    pub fn intersects_block_aabb(&self, other: &AABB) -> bool {
        self.max_x > f64::from(other.min_x)
            && self.min_x < f64::from(other.max_x)
            && self.max_y > f64::from(other.min_y)
            && self.min_y < f64::from(other.max_y)
            && self.max_z > f64::from(other.min_z)
            && self.min_z < f64::from(other.max_z)
    }
}

/// A VoxelShape is a collection of AABBs that define the shape of a block.
///
/// For simple blocks, this is typically a single AABB (full block or empty).
/// For complex blocks like stairs or fences, this is multiple AABBs combined.
pub type VoxelShape = &'static [AABB];

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
    shapes: Vec<&'static [AABB]>,
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
        let empty_id = registry.register(&[]);
        debug_assert_eq!(empty_id, ShapeId::EMPTY);

        let full_id = registry.register(FULL_BLOCK_SHAPE);
        debug_assert_eq!(full_id, ShapeId::FULL_BLOCK);

        registry
    }

    /// Registers a new shape and returns its ID.
    ///
    /// # Panics
    /// Panics if the registry has been frozen.
    pub fn register(&mut self, shape: &'static [AABB]) -> ShapeId {
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
    /// Returns an empty slice if the ID is invalid.
    #[must_use]
    pub fn get(&self, id: ShapeId) -> &'static [AABB] {
        self.shapes.get(id.0 as usize).copied().unwrap_or(&[])
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

// Static shape for full block - used during registry initialization
static FULL_BLOCK_SHAPE: &[AABB] = &[AABB::FULL_BLOCK];

/// Shape data for a block state, containing both collision and outline shapes.
#[derive(Debug, Clone, Copy)]
pub struct BlockShapes {
    pub collision: VoxelShape,
    pub outline: VoxelShape,
}

impl BlockShapes {
    /// Creates new block shapes.
    #[must_use]
    pub const fn new(collision: VoxelShape, outline: VoxelShape) -> Self {
        Self { collision, outline }
    }

    /// Full block collision and outline.
    pub const FULL_BLOCK: BlockShapes = BlockShapes::new(&[AABB::FULL_BLOCK], &[AABB::FULL_BLOCK]);

    /// Empty shapes (no collision, no outline).
    pub const EMPTY: BlockShapes = BlockShapes::new(&[], &[]);
}

use super::properties::Direction;

/// Checks if a shape is a full block (covers the entire 0-1 cube).
///
/// This matches vanilla's `Block.isShapeFullBlock()` used by `isSolid()`.
#[must_use]
pub fn is_shape_full_block(shape: VoxelShape) -> bool {
    // A full block shape must have exactly one AABB that covers 0-1 on all axes
    if shape.len() != 1 {
        return false;
    }

    let aabb = &shape[0];
    aabb.min_x <= 0.0
        && aabb.max_x >= 1.0
        && aabb.min_y <= 0.0
        && aabb.max_y >= 1.0
        && aabb.min_z <= 0.0
        && aabb.max_z >= 1.0
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

/// Center support shape: a 4x4 pixel column in the center (2-14 in pixel coords = 0.125-0.875).
const CENTER_SUPPORT_MIN: f32 = 0.125; // 2/16
const CENTER_SUPPORT_MAX: f32 = 0.875; // 14/16

/// Rigid support requires coverage except for a 2-pixel border.
const RIGID_BORDER: f32 = 0.125; // 2/16

/// Checks if a shape fully covers a face (for `SupportType::Full`).
///
/// Returns true if the 2D projection of the shape on the given face
/// completely covers the 1x1 face area.
#[must_use]
pub fn is_face_full(shape: VoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    // For a face to be "full", the shape's projection onto that face must cover 0.0-1.0
    // on both axes perpendicular to the direction.
    match direction {
        Direction::Down => covers_face_xy(shape, |aabb| aabb.min_y <= 0.0),
        Direction::Up => covers_face_xy(shape, |aabb| aabb.max_y >= 1.0),
        Direction::North => covers_face_xy_for_z(shape, |aabb| aabb.min_z <= 0.0),
        Direction::South => covers_face_xy_for_z(shape, |aabb| aabb.max_z >= 1.0),
        Direction::West => covers_face_yz(shape, |aabb| aabb.min_x <= 0.0),
        Direction::East => covers_face_yz(shape, |aabb| aabb.max_x >= 1.0),
    }
}

/// Checks if a shape provides center support on a face.
///
/// The center area is a 12x12 pixel region (0.125 to 0.875 on each axis).
#[must_use]
pub fn is_face_center_supported(shape: VoxelShape, direction: Direction) -> bool {
    if shape.is_empty() {
        return false;
    }

    // Check if any AABB in the shape covers the center region on the given face
    match direction {
        Direction::Down => shape.iter().any(|aabb| {
            aabb.min_y <= 0.0
                && aabb.min_x <= CENTER_SUPPORT_MIN
                && aabb.max_x >= CENTER_SUPPORT_MAX
                && aabb.min_z <= CENTER_SUPPORT_MIN
                && aabb.max_z >= CENTER_SUPPORT_MAX
        }),
        Direction::Up => shape.iter().any(|aabb| {
            aabb.max_y >= 1.0
                && aabb.min_x <= CENTER_SUPPORT_MIN
                && aabb.max_x >= CENTER_SUPPORT_MAX
                && aabb.min_z <= CENTER_SUPPORT_MIN
                && aabb.max_z >= CENTER_SUPPORT_MAX
        }),
        Direction::North => shape.iter().any(|aabb| {
            aabb.min_z <= 0.0
                && aabb.min_x <= CENTER_SUPPORT_MIN
                && aabb.max_x >= CENTER_SUPPORT_MAX
                && aabb.min_y <= CENTER_SUPPORT_MIN
                && aabb.max_y >= CENTER_SUPPORT_MAX
        }),
        Direction::South => shape.iter().any(|aabb| {
            aabb.max_z >= 1.0
                && aabb.min_x <= CENTER_SUPPORT_MIN
                && aabb.max_x >= CENTER_SUPPORT_MAX
                && aabb.min_y <= CENTER_SUPPORT_MIN
                && aabb.max_y >= CENTER_SUPPORT_MAX
        }),
        Direction::West => shape.iter().any(|aabb| {
            aabb.min_x <= 0.0
                && aabb.min_y <= CENTER_SUPPORT_MIN
                && aabb.max_y >= CENTER_SUPPORT_MAX
                && aabb.min_z <= CENTER_SUPPORT_MIN
                && aabb.max_z >= CENTER_SUPPORT_MAX
        }),
        Direction::East => shape.iter().any(|aabb| {
            aabb.max_x >= 1.0
                && aabb.min_y <= CENTER_SUPPORT_MIN
                && aabb.max_y >= CENTER_SUPPORT_MAX
                && aabb.min_z <= CENTER_SUPPORT_MIN
                && aabb.max_z >= CENTER_SUPPORT_MAX
        }),
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
            aabb.min_y <= 0.0
                && aabb.min_x <= min_bound
                && aabb.max_x >= max_bound
                && aabb.min_z <= min_bound
                && aabb.max_z >= max_bound
        }),
        Direction::Up => shape.iter().any(|aabb| {
            aabb.max_y >= 1.0
                && aabb.min_x <= min_bound
                && aabb.max_x >= max_bound
                && aabb.min_z <= min_bound
                && aabb.max_z >= max_bound
        }),
        Direction::North => shape.iter().any(|aabb| {
            aabb.min_z <= 0.0
                && aabb.min_x <= min_bound
                && aabb.max_x >= max_bound
                && aabb.min_y <= min_bound
                && aabb.max_y >= max_bound
        }),
        Direction::South => shape.iter().any(|aabb| {
            aabb.max_z >= 1.0
                && aabb.min_x <= min_bound
                && aabb.max_x >= max_bound
                && aabb.min_y <= min_bound
                && aabb.max_y >= max_bound
        }),
        Direction::West => shape.iter().any(|aabb| {
            aabb.min_x <= 0.0
                && aabb.min_y <= min_bound
                && aabb.max_y >= max_bound
                && aabb.min_z <= min_bound
                && aabb.max_z >= max_bound
        }),
        Direction::East => shape.iter().any(|aabb| {
            aabb.max_x >= 1.0
                && aabb.min_y <= min_bound
                && aabb.max_y >= max_bound
                && aabb.min_z <= min_bound
                && aabb.max_z >= max_bound
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

// Helper: checks if shape covers full X-Y face (for Up/Down directions)
fn covers_face_xy(shape: VoxelShape, face_check: impl Fn(&AABB) -> bool) -> bool {
    // Simple check: if there's a single AABB that covers 0-1 on X and Z and touches the face
    shape.iter().any(|aabb| {
        face_check(aabb)
            && aabb.min_x <= 0.0
            && aabb.max_x >= 1.0
            && aabb.min_z <= 0.0
            && aabb.max_z >= 1.0
    })
}

// Helper: checks if shape covers full X-Y face (for North/South directions)
fn covers_face_xy_for_z(shape: VoxelShape, face_check: impl Fn(&AABB) -> bool) -> bool {
    shape.iter().any(|aabb| {
        face_check(aabb)
            && aabb.min_x <= 0.0
            && aabb.max_x >= 1.0
            && aabb.min_y <= 0.0
            && aabb.max_y >= 1.0
    })
}

// Helper: checks if shape covers full Y-Z face (for East/West directions)
fn covers_face_yz(shape: VoxelShape, face_check: impl Fn(&AABB) -> bool) -> bool {
    shape.iter().any(|aabb| {
        face_check(aabb)
            && aabb.min_y <= 0.0
            && aabb.max_y >= 1.0
            && aabb.min_z <= 0.0
            && aabb.max_z >= 1.0
    })
}
