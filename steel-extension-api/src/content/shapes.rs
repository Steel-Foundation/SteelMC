use stabby::vec::Vec as ApiVec;

/// A box in vanilla's 0–16 block-model coordinate system.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct ShapeBox {
    /// Minimum XYZ bounds, in sixteenths of a block.
    pub min: [f64; 3],
    /// Maximum XYZ bounds, in sixteenths of a block.
    pub max: [f64; 3],
}

/// A union of boxes. Coordinates are validated during registration.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct VoxelShape {
    /// Boxes combined by union.
    pub boxes: ApiVec<ShapeBox>,
}

impl VoxelShape {
    /// Creates a shape with no boxes.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            boxes: ApiVec::new(),
        }
    }

    /// Creates a full cube.
    #[must_use]
    pub fn block() -> Self {
        Self::box_shape(0.0, 0.0, 0.0, 16.0, 16.0, 16.0)
    }

    /// Creates one box using vanilla 0–16 coordinates.
    #[must_use]
    pub fn box_shape(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Self {
        Self {
            boxes: [ShapeBox {
                min: [x1, y1, z1],
                max: [x2, y2, z2],
            }]
            .into_iter()
            .collect(),
        }
    }

    /// Combines both shapes by union.
    #[must_use]
    pub fn or(mut self, other: Self) -> Self {
        self.boxes.extend(other.boxes);
        self
    }
}

/// Independent server shape channels, matching Steel's native block definition.
#[stabby::stabby]
#[derive(Debug, Clone)]
pub struct BlockShapes {
    /// Physical collision.
    pub collision: VoxelShape,
    /// Support for neighboring blocks.
    pub support: VoxelShape,
    /// Targeting outline.
    pub outline: VoxelShape,
    /// Face occlusion.
    pub occlusion: VoxelShape,
    /// Additional interaction geometry.
    pub interaction: VoxelShape,
    /// Visual obstruction geometry.
    pub visual: VoxelShape,
}

impl BlockShapes {
    /// Shares the shape across the main channels; interaction is empty.
    #[must_use]
    pub fn new(shape: VoxelShape) -> Self {
        Self {
            collision: shape.clone(),
            support: shape.clone(),
            outline: shape.clone(),
            occlusion: shape.clone(),
            visual: shape,
            interaction: VoxelShape::empty(),
        }
    }
}
