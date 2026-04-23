//! 3D pattern matcher for multiblock structures (end portal, wither, golems, beacon base).
//!
//! Deviations from vanilla's `BlockPattern`:
//!
//! 1. Chars are validated at builder/macro time (see [`block_pattern!`]), not at match time.
//! 2. We only try the 4 horizontal Y-axis rotations, not vanilla's 24 orientations.
//!
//! Predicates are evaluated against the actual world state — the pattern only rotates
//! *coordinates*, not states. Rotation-sensitive predicates match only in canonical
//! orientation; rotation-invariant ones (skulls, soul sand) match in any aligned rotation.

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
    /// Row-major: index = (y * depth + z) * width + x.
    cells: Box<[BlockPredicate]>,
}

/// Horizontal rotation around the Y axis. Canonical orientation: X = east, Z = south.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// 0° (canonical).
    None,
    /// 90° clockwise.
    Cw90,
    /// 180°.
    Cw180,
    /// 270° clockwise.
    Cw270,
}

impl Rotation {
    /// All four rotations in CW order.
    pub const ALL: [Rotation; 4] = [Self::None, Self::Cw90, Self::Cw180, Self::Cw270];

    /// Rotates a `(dx, dz)` offset around the Y axis.
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
    origin: BlockPos,
    rotation: Rotation,
    width: u32,
    height: u32,
    depth: u32,
}

impl BlockPatternMatch {
    /// World position of pattern cell `(0, 0, 0)`.
    #[must_use]
    pub const fn origin(&self) -> BlockPos {
        self.origin
    }

    /// Rotation applied to find this match.
    #[must_use]
    pub const fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// Canonical X width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Y height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Canonical Z depth.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    /// World position of pattern cell `(px, py, pz)`.
    #[must_use]
    pub fn pos_at(&self, px: u32, py: u32, pz: u32) -> BlockPos {
        let (dx, dz) = self.rotation.rotate_xz(px as i32, pz as i32);
        self.origin.offset(dx, py as i32, dz)
    }
}

impl BlockPattern {
    pub(super) fn from_raw(
        width: u32,
        height: u32,
        depth: u32,
        cells: Box<[BlockPredicate]>,
    ) -> Self {
        debug_assert_eq!(cells.len(), (width * height * depth) as usize);
        Self { width, height, depth, cells }
    }

    /// Canonical X width.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Y height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Canonical Z depth.
    #[must_use]
    pub const fn depth(&self) -> u32 {
        self.depth
    }

    fn cell(&self, x: u32, y: u32, z: u32) -> &BlockPredicate {
        &self.cells[((y * self.depth + z) * self.width + x) as usize]
    }

    /// Checks whether the pattern matches with cell `(0,0,0)` at `origin` and `rotation`.
    #[must_use]
    pub fn matches_at(&self, world: &World, origin: BlockPos, rotation: Rotation) -> bool {
        for y in 0..self.height {
            for z in 0..self.depth {
                for x in 0..self.width {
                    let (dx, dz) = rotation.rotate_xz(x as i32, z as i32);
                    let state = world.get_block_state(origin.offset(dx, y as i32, dz));
                    if !self.cell(x, y, z).matches(state) {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Searches for a match containing `hint`, trying every (cell, rotation) pair.
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

/// Builds a [`BlockPattern`] from a legend and aisle grid. Multiple Y layers are
/// separated by `---`.
///
/// ```ignore
/// block_pattern! {
///     '?' => BlockPredicate::Any,
///     'v' => frame_eye(Direction::North),
///     ;
///     "?vvv?",
///     ">???<",
///     ">???<",
///     "?^^^?",
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

/// Rotation such that the pattern's canonical "south" (`+Z`) points in `dir`.
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
