//! Template pool and structure template data for jigsaw assembly.
//!
//! Parsed at build time from vanilla datapack JSONs and structure NBT files.
//! Used by the jigsaw placement system to assemble structures from pools.

use steel_utils::{Direction, Identifier, Rotation};

/// Orientation of a jigsaw block, encoding both facing direction and up direction.
///
/// Vanilla's `FrontAndTop` enum — the orientation block state property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JigsawOrientation {
    DownEast,
    DownNorth,
    DownSouth,
    DownWest,
    UpEast,
    UpNorth,
    UpSouth,
    UpWest,
    WestUp,
    EastUp,
    NorthUp,
    SouthUp,
}

impl JigsawOrientation {
    /// Parses from the block state property string (e.g., `"up_north"`).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "down_east" => Self::DownEast,
            "down_north" => Self::DownNorth,
            "down_south" => Self::DownSouth,
            "down_west" => Self::DownWest,
            "up_east" => Self::UpEast,
            "up_north" => Self::UpNorth,
            "up_south" => Self::UpSouth,
            "up_west" => Self::UpWest,
            "west_up" => Self::WestUp,
            "east_up" => Self::EastUp,
            "north_up" => Self::NorthUp,
            "south_up" => Self::SouthUp,
            _ => return None,
        })
    }

    /// Returns the front-facing direction.
    #[must_use]
    pub const fn front_direction(self) -> Direction {
        match self {
            Self::DownEast | Self::DownNorth | Self::DownSouth | Self::DownWest => Direction::Down,
            Self::UpEast | Self::UpNorth | Self::UpSouth | Self::UpWest => Direction::Up,
            Self::WestUp => Direction::West,
            Self::EastUp => Direction::East,
            Self::NorthUp => Direction::North,
            Self::SouthUp => Direction::South,
        }
    }

    /// Returns the top direction.
    #[must_use]
    pub const fn top_direction(self) -> Direction {
        match self {
            Self::DownEast | Self::UpEast => Direction::East,
            Self::DownNorth | Self::UpNorth => Direction::North,
            Self::DownSouth | Self::UpSouth => Direction::South,
            Self::DownWest | Self::UpWest => Direction::West,
            Self::WestUp | Self::EastUp | Self::NorthUp | Self::SouthUp => Direction::Up,
        }
    }

    /// Returns the front-facing direction offset as (dx, dy, dz).
    #[must_use]
    pub fn front(self) -> (i32, i32, i32) {
        self.front_direction().offset()
    }

    /// Constructs an orientation from front and top directions.
    ///
    /// Returns `None` if the combination is invalid.
    #[must_use]
    pub const fn from_directions(front: Direction, top: Direction) -> Option<Self> {
        // Match vanilla's FrontAndTop lookup table
        Some(match (front, top) {
            (Direction::Down, Direction::East) => Self::DownEast,
            (Direction::Down, Direction::North) => Self::DownNorth,
            (Direction::Down, Direction::South) => Self::DownSouth,
            (Direction::Down, Direction::West) => Self::DownWest,
            (Direction::Up, Direction::East) => Self::UpEast,
            (Direction::Up, Direction::North) => Self::UpNorth,
            (Direction::Up, Direction::South) => Self::UpSouth,
            (Direction::Up, Direction::West) => Self::UpWest,
            (Direction::West, Direction::Up) => Self::WestUp,
            (Direction::East, Direction::Up) => Self::EastUp,
            (Direction::North, Direction::Up) => Self::NorthUp,
            (Direction::South, Direction::Up) => Self::SouthUp,
            _ => return None,
        })
    }

    /// Rotates this orientation by the given rotation around the Y axis.
    ///
    /// Both the front and top directions are rotated, matching vanilla's
    /// `BlockState.rotate(rotation)` for jigsaw blocks.
    #[must_use]
    pub fn rotate(self, rotation: Rotation) -> Self {
        let front = rotation.rotate(self.front_direction());
        let top = rotation.rotate(self.top_direction());
        // Rotation of valid FrontAndTop always produces a valid FrontAndTop
        Self::from_directions(front, top).expect("rotated orientation should be valid")
    }
}

/// Joint type for jigsaw connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JointType {
    /// Can rotate freely around the connection axis.
    Rollable,
    /// Must maintain alignment with the source piece.
    Aligned,
}

/// A jigsaw connector block extracted from a structure template.
#[derive(Debug, Clone)]
pub struct JigsawBlock {
    /// Position relative to template origin.
    pub pos: [i32; 3],
    /// Orientation (determines facing direction).
    pub orientation: JigsawOrientation,
    /// Name of this jigsaw connector.
    pub name: Identifier,
    /// Target connector name to attach to.
    pub target: Identifier,
    /// Pool to draw target pieces from.
    pub pool: Identifier,
    /// Joint type.
    pub joint: JointType,
    /// Block state to replace jigsaw with after placement.
    pub final_state: Identifier,
    /// Priority for selecting this jigsaw among siblings in a piece (higher = tried first).
    pub selection_priority: i32,
    /// Priority for BFS queue ordering when placing children (higher = processed first).
    pub placement_priority: i32,
}

/// Extracted data from a structure template NBT file.
///
/// Contains only the information needed for jigsaw assembly — not the full
/// block data (which is loaded separately for actual placement).
#[derive(Debug, Clone)]
pub struct TemplateData {
    /// Template size in blocks (x, y, z).
    pub size: [i32; 3],
    /// Jigsaw connector blocks in this template.
    pub jigsaws: Vec<JigsawBlock>,
}

/// Projection mode for pool elements.
///
/// Vanilla's `StructureTemplatePool.Projection`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Projection {
    /// Fixed Y position, stacked based on jigsaw positions.
    Rigid,
    /// Adjusts vertically to match terrain surface.
    TerrainMatching,
}

impl Projection {
    /// Returns the ground level delta for this projection.
    ///
    /// Vanilla's `StructurePoolElement.getGroundLevelDelta()` returns 1 by default.
    /// This is the offset from the piece's minY to ground level.
    #[must_use]
    pub const fn ground_level_delta(self) -> i32 {
        1
    }
}

/// Vanilla's `Holder<StructureProcessorList>` on single pool elements.
///
/// The generated vanilla datapack currently uses registry references plus direct
/// empty lists. If direct non-empty lists show up, codegen should preserve their
/// processor entries instead of flattening them into this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessorList {
    /// Direct empty processor list: `{ "processors": [] }`.
    Empty,
    /// Registry-backed processor list, e.g. `minecraft:street_savanna`.
    Registry(Identifier),
}

/// A pool element — one possible piece that can be drawn from a template pool.
#[derive(Debug, Clone)]
pub enum PoolElement {
    /// Single structure template piece.
    Single {
        /// Template location (e.g., `minecraft:village/plains/houses/small_house_1`).
        location: Identifier,
        /// Processors applied during block placement.
        processors: ProcessorList,
        /// Vertical placement mode.
        projection: Projection,
    },
    /// Legacy single piece (same as Single but uses legacy jigsaw processing).
    LegacySingle {
        /// Template location.
        location: Identifier,
        /// Processors applied during block placement.
        processors: ProcessorList,
        /// Vertical placement mode.
        projection: Projection,
    },
    /// Empty placeholder element — signals no piece should be placed.
    Empty,
    /// A placed feature (not a structure template).
    Feature {
        /// Feature identifier.
        feature: Identifier,
        /// Vertical placement mode.
        projection: Projection,
    },
    /// A list of elements placed as a group.
    List {
        /// Sub-elements.
        elements: Vec<PoolElement>,
        /// Vertical placement mode.
        projection: Projection,
    },
}

impl PoolElement {
    /// Returns the projection mode, or `Rigid` for empty elements.
    #[must_use]
    pub fn projection(&self) -> Projection {
        match self {
            Self::Single { projection, .. }
            | Self::LegacySingle { projection, .. }
            | Self::Feature { projection, .. }
            | Self::List { projection, .. } => *projection,
            Self::Empty => Projection::Rigid,
        }
    }

    /// Returns true if this is an empty placeholder element.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
}

/// A template pool — a collection of weighted pool elements.
///
/// Vanilla's `StructureTemplatePool`.
#[derive(Debug, Clone)]
pub struct TemplatePoolData {
    /// Registry key (e.g., `minecraft:village/plains/town_centers`).
    pub key: Identifier,
    /// Fallback pool used when the main pool is exhausted.
    pub fallback: Identifier,
    /// Weighted elements. Each entry is (element, weight).
    pub elements: Vec<(PoolElement, i32)>,
}
