//! 3D pattern matcher for multiblock structures (end portal, wither, golems, beacon base).
//!
//! Vanilla parity: `net.minecraft.world.level.block.state.pattern.BlockPattern` and
//! `BlockPatternBuilder`, with two intentional deviations:
//!
//! 1. Vanilla's char-aisle DSL is preserved (most ergonomic for visual layouts), but
//!    chars are validated against the legend at builder/macro time, not at match time.
//!    See [`builder::BlockPatternBuilder`] and [`block_pattern!`].
//! 2. We only try the 4 horizontal Y-axis rotations, not vanilla's 24 orientations.
//!    All current callers (end portal, wither, golems) have a fixed up direction;
//!    arbitrary-axis matching can be added later without breaking the public API.
//!
//! Like vanilla, predicates are evaluated against the actual world block state — the
//! pattern only rotates *coordinates*, not states. This means rotation-sensitive
//! predicates (e.g. "frame facing north") only match when the world is in the pattern's
//! canonical orientation. Patterns with rotational symmetry (end portal) match in
//! every rotation; rotation-invariant predicates (skulls, soul sand) match in any
//! rotation that aligns the layout.

mod builder;
mod predicate;

pub use builder::{BlockPatternBuilder, BlockPatternBuildError};
pub use predicate::BlockPredicate;

use steel_utils::{BlockPos, Direction};

use crate::world::World;

/// 3D grid of [`BlockPredicate`]s. Construct via [`BlockPatternBuilder`].
#[derive(Debug)]
pub struct BlockPattern {
    width: u32,
    height: u32,
    depth: u32,
    /// Flat row-major: index = (y * depth + z) * width + x.
    cells: Box<[BlockPredicate]>,
}

/// Horizontal rotation around the Y axis.
///
/// Pattern coordinates are written assuming `None` (canonical orientation): X = east,
/// Z = south. Other rotations rotate pattern coordinates clockwise when viewed from
/// above.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// 0° — canonical orientation. Pattern X = world east, pattern Z = world south.
    None,
    /// 90° clockwise (viewed from above). Pattern X → world south.
    Cw90,
    /// 180°. Pattern X → world west.
    Cw180,
    /// 270° clockwise (= 90° counter-clockwise). Pattern X → world north.
    Cw270,
}

impl Rotation {
    /// All four horizontal rotations in CW order.
    pub const ALL: [Rotation; 4] = [Self::None, Self::Cw90, Self::Cw180, Self::Cw270];

    /// Rotates a (dx, dz) offset around the Y axis.
    #[must_use]
    pub const fn rotate_xz(self, dx: i32, dz: i32) -> (i32, i32) {
        match self {
            Self::None => (dx, dz),
            Self::Cw90 => (-dz, dx),
            Self::Cw180 => (-dx, -dz),
            Self::Cw270 => (dz, -dx),
        }
    }
}

/// A successful match returned by [`BlockPattern::find`].
#[derive(Debug, Clone)]
pub struct BlockPatternMatch {
    /// World position of pattern cell `(0, 0, 0)`.
    origin: BlockPos,
    /// Rotation applied to pattern coordinates.
    rotation: Rotation,
    /// Pattern dimensions, for navigating cells of the match.
    width: u32,
    height: u32,
    depth: u32,
}

impl BlockPatternMatch {
    /// World position of pattern cell `(0, 0, 0)` (canonical pattern origin).
    #[must_use]
    pub const fn origin(&self) -> BlockPos {
        self.origin
    }

    /// Rotation that was applied to find this match.
    #[must_use]
    pub const fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Pattern width along canonical X.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pattern height along Y.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Pattern depth along canonical Z.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// World position of the given pattern cell `(px, py, pz)` for this match.
    #[must_use]
    pub fn pos_at(&self, px: u32, py: u32, pz: u32) -> BlockPos {
        let (dx, dz) = self.rotation.rotate_xz(px as i32, pz as i32);
        self.origin.offset(dx, py as i32, dz)
    }
}

impl BlockPattern {
    /// Internal constructor used by the builder.
    pub(super) fn from_raw(
        width: u32,
        height: u32,
        depth: u32,
        cells: Box<[BlockPredicate]>,
    ) -> Self {
        debug_assert_eq!(cells.len(), (width * height * depth) as usize);
        Self {
            width,
            height,
            depth,
            cells,
        }
    }

    /// Pattern width (canonical X).
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Pattern height (Y).
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Pattern depth (canonical Z).
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// Predicate at canonical pattern position `(x, y, z)`.
    fn cell(&self, x: u32, y: u32, z: u32) -> &BlockPredicate {
        let idx = ((y * self.depth + z) * self.width + x) as usize;
        &self.cells[idx]
    }

    /// Checks whether the pattern matches the world with cell `(0, 0, 0)` placed at
    /// `origin` and the given `rotation`.
    #[must_use]
    pub fn matches_at(&self, world: &World, origin: BlockPos, rotation: Rotation) -> bool {
        for y in 0..self.height {
            for z in 0..self.depth {
                for x in 0..self.width {
                    let (dx, dz) = rotation.rotate_xz(x as i32, z as i32);
                    let world_pos = origin.offset(dx, y as i32, dz);
                    let state = world.get_block_state(world_pos);
                    if !self.cell(x, y, z).matches(state) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Searches for a match where `hint` is contained in the pattern.
    ///
    /// Tries every (cell, rotation) combination, deriving the origin so that `hint`
    /// lands on the chosen cell. Returns the first match found.
    #[must_use]
    pub fn find(&self, world: &World, hint: BlockPos) -> Option<BlockPatternMatch> {
        for rotation in Rotation::ALL {
            for py in 0..self.height {
                for pz in 0..self.depth {
                    for px in 0..self.width {
                        let (dx, dz) = rotation.rotate_xz(px as i32, pz as i32);
                        let origin = hint.offset(-dx, -(py as i32), -dz);
                        if self.matches_at(world, origin, rotation) {
                            return Some(BlockPatternMatch {
                                origin,
                                rotation,
                                width: self.width,
                                height: self.height,
                                depth: self.depth,
                            });
                        }
                    }
                }
            }
        }
        None
    }
}

/// Builds a [`BlockPattern`] from a legend and an aisle grid, validating chars at
/// macro expansion time (unknown chars become builder errors at runtime, but the
/// macro form makes typos easier to spot).
///
/// Syntax:
/// ```ignore
/// block_pattern! {
///     '?' => BlockPredicate::Any,
///     'v' => frame_eye(Direction::North),
///     // ...
///     ;
///     "?vvv?",
///     ">???<",
///     ">???<",
///     ">???<",
///     "?^^^?",
/// }
/// ```
///
/// Multiple Y layers are separated by `---`:
/// ```ignore
/// block_pattern! {
///     '#' => BlockPredicate::Block(&vanilla_blocks::IRON_BLOCK),
///     '?' => BlockPredicate::Any,
///     ;
///     "###",
///     ---
///     "?#?",
/// }
/// ```
#[macro_export]
macro_rules! block_pattern {
    (
        $( $sym:literal => $pred:expr ),+ $(,)?
        ;
        $( $row:literal ),+ $(,)?
    ) => {{
        $crate::world::block_pattern::BlockPatternBuilder::new()
            .aisle(&[ $( $row ),+ ])
            $( .symbol($sym, $pred) )+
            .build()
            .expect("static block_pattern! is well-formed")
    }};
    (
        $( $sym:literal => $pred:expr ),+ $(,)?
        ;
        $( $( $row:literal ),+ $(,)? )---+
    ) => {{
        $crate::world::block_pattern::BlockPatternBuilder::new()
            $( .aisle(&[ $( $row ),+ ]) )+
            $( .symbol($sym, $pred) )+
            .build()
            .expect("static block_pattern! is well-formed")
    }};
}

/// Maps a horizontal [`Direction`] (north/south/east/west) to the equivalent
/// [`Rotation`] applied to the pattern such that pattern's canonical "south"
/// (positive Z) ends up pointing in `dir`. Useful for callers that have a
/// known directional intent.
#[must_use]
pub const fn rotation_for_facing(dir: Direction) -> Option<Rotation> {
    match dir {
        Direction::South => Some(Rotation::None),
        Direction::West => Some(Rotation::Cw90),
        Direction::North => Some(Rotation::Cw180),
        Direction::East => Some(Rotation::Cw270),
        _ => None,
    }
}
