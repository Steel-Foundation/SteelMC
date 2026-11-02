//! Fast region fills ([`World::fill`](crate::World::fill)).
//!
//! A fill writes whole chunk sections at once without physics or block callbacks, the way
//! Paper prison and editing plugins do, at roughly ten million blocks per second. Every
//! client in range sees the result on the next tick and light is recomputed in the normal
//! light pass. The [`FillReport`] lists what was replaced, so "break this layer and sell it"
//! is one fill with air plus the histogram.
//!
//! A fill is refused, with nothing written, when any chunk it touches is not loaded.

use crate::{BlockPos, BlockState};
use stabby::vec::Vec as ApiVec;

/// Region kind of a [`FillRequest`].
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillShapeKind {
    /// Inclusive box between `min` and `max`.
    Cuboid,
    /// Ball of `radius` around `center`.
    Sphere,
    /// Outer `thickness` blocks of a ball of `radius` around `center`.
    Shell,
}

/// Which existing blocks a fill may replace.
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FillMode {
    /// Every block in the region.
    #[default]
    All,
    /// Only air (refill without overwriting).
    OnlyAir,
    /// Only non-air (clear or replace what is there).
    NonAir,
}

/// One state of a fill pattern with its relative weight.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternEntry {
    /// State to write.
    pub state: BlockState,
    /// Relative weight; zero entries are ignored.
    pub weight: u32,
}

/// A fill request; build it with [`FillRequest::cuboid`], [`FillRequest::sphere`],
/// [`FillRequest::shell`] or [`FillRequest::layer`].
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq)]
pub struct FillRequest {
    /// Weighted states; each block picks one at random.
    pub pattern: ApiVec<PatternEntry>,
    /// Sphere/shell radius.
    pub radius: f64,
    /// Shell thickness.
    pub thickness: f64,
    /// Cuboid corner.
    pub min: BlockPos,
    /// Opposite cuboid corner.
    pub max: BlockPos,
    /// Sphere/shell center.
    pub center: BlockPos,
    /// Clip box corner (used when `clipped`).
    pub clip_min: BlockPos,
    /// Opposite clip box corner.
    pub clip_max: BlockPos,
    /// Region kind.
    pub kind: FillShapeKind,
    /// Replace mode.
    pub mode: FillMode,
    /// Whether the region is clipped to `clip_min..=clip_max`.
    pub clipped: bool,
}

const ORIGIN: BlockPos = BlockPos { x: 0, y: 0, z: 0 };

impl FillRequest {
    fn new(kind: FillShapeKind, pattern: &[PatternEntry]) -> Self {
        Self {
            pattern: pattern.iter().copied().collect(),
            radius: 0.0,
            thickness: 0.0,
            min: ORIGIN,
            max: ORIGIN,
            center: ORIGIN,
            clip_min: ORIGIN,
            clip_max: ORIGIN,
            kind,
            mode: FillMode::All,
            clipped: false,
        }
    }

    /// The inclusive box between two corners.
    #[must_use]
    pub fn cuboid(min: BlockPos, max: BlockPos, pattern: &[PatternEntry]) -> Self {
        Self {
            min,
            max,
            ..Self::new(FillShapeKind::Cuboid, pattern)
        }
    }

    /// One horizontal layer at `y` of the box between two corners.
    #[must_use]
    pub fn layer(min: BlockPos, max: BlockPos, y: i32, pattern: &[PatternEntry]) -> Self {
        Self::cuboid(BlockPos { y, ..min }, BlockPos { y, ..max }, pattern)
    }

    /// A ball of `radius` blocks.
    #[must_use]
    pub fn sphere(center: BlockPos, radius: f64, pattern: &[PatternEntry]) -> Self {
        Self {
            radius,
            center,
            ..Self::new(FillShapeKind::Sphere, pattern)
        }
    }

    /// The outer `thickness` blocks of a ball of `radius` blocks.
    #[must_use]
    pub fn shell(center: BlockPos, radius: f64, thickness: f64, pattern: &[PatternEntry]) -> Self {
        Self {
            radius,
            thickness,
            center,
            ..Self::new(FillShapeKind::Shell, pattern)
        }
    }

    /// Limits the region to the box between two corners.
    #[must_use]
    pub const fn clip(mut self, min: BlockPos, max: BlockPos) -> Self {
        self.clip_min = min;
        self.clip_max = max;
        self.clipped = true;
        self
    }

    /// Sets the replace mode.
    #[must_use]
    pub const fn mode(mut self, mode: FillMode) -> Self {
        self.mode = mode;
        self
    }
}

/// How often one state was replaced.
#[stabby::stabby]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacedCount {
    /// Number of blocks.
    pub count: u64,
    /// State that was there before.
    pub state: BlockState,
}

/// Outcome of a fill.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FillReport {
    /// Replaced states, most frequent first.
    pub replaced: ApiVec<ReplacedCount>,
    /// Blocks that changed.
    pub changed: u64,
    /// Selected blocks that already had the chosen state.
    pub unchanged: u64,
    /// Write time in microseconds (light propagation follows in the next ticks).
    pub micros: u64,
    /// Chunks touched.
    pub chunks: u32,
    /// Chunk sections written.
    pub sections: u32,
}

impl FillReport {
    /// Blocks written per second.
    #[must_use]
    pub fn blocks_per_second(&self) -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a rate does not need exact integer precision"
        )]
        let (changed, micros) = (self.changed as f64, self.micros.max(1) as f64);
        changed * 1_000_000.0 / micros
    }

    /// How many blocks of `state` were replaced.
    #[must_use]
    pub fn replaced_count(&self, state: BlockState) -> u64 {
        self.replaced
            .iter()
            .find(|entry| entry.state == state)
            .map_or(0, |entry| entry.count)
    }
}

/// Which existing blocks [`World::replace_blocks`](crate::World::replace_blocks) replaces.
#[stabby::stabby]
#[derive(Debug, Clone, Default)]
pub struct ReplaceFilter {
    /// Exact states to replace.
    pub states: ApiVec<BlockState>,
    /// Also replace every state vanilla lets placement replace (air, plants, fluids).
    pub replaceable: bool,
}

#[stabby::host_api]
/// Fill bindings on `World`.
pub mod api {
    use super::{FillReport, FillRequest};
    use crate::{ApiResult, World};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;

    #[cfg(feature = "host")]
    use crate::host::fill as host;

    impl<'a> World<'a> {
        /// Fills a region with section-batched writes; see the [module docs](crate::fill).
        pub fn fill(&self, request: FillRequest) -> ApiResult<FillReport> {
            host::fill(self.host(), &request)
                .map_err(ApiString::from)
                .into()
        }
    }
}

#[stabby::host_api]
/// Filtered replacement, a separate module so the `fill` binding stays unchanged.
pub mod replace_api {
    use super::{FillReport, ReplaceFilter};
    use crate::{ApiResult, BlockPos, BlockState, World};
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;

    #[cfg(feature = "host")]
    use crate::host::fill as host;

    impl<'a> World<'a> {
        /// Replaces the blocks of the inclusive box that `filter` selects with `state`,
        /// through the same section-batched path as [`World::fill`].
        pub fn replace_blocks(
            &self,
            min: BlockPos,
            max: BlockPos,
            filter: ReplaceFilter,
            state: BlockState,
        ) -> ApiResult<FillReport> {
            host::replace_blocks(self.host(), min, max, &filter, state)
                .map_err(ApiString::from)
                .into()
        }
    }
}
