//! Spread calculation context for fluid flow optimization.
//!
//! Based on vanilla's FlowingFluid.SpreadContext, this provides local caching
//! of block states and hole checks during fluid spread calculations.
//!
//! This avoids repeatedly querying the world for the same positions during
//! the recursive slope-finding algorithm.

use rustc_hash::FxHashMap;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::fluid::FluidRef;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::world::World;
use crate::fluid::collision::can_pass_horizontally_internal;
/// Context for fluid spread calculations with local caching.
///
/// This is created fresh for each `get_spread()` call and caches:
/// - `BlockState` lookups by relative position
/// - Hole check results by relative position
///
/// # Parity note
/// Cache keys are encoded as **relative offsets** from the spread origin,
/// mirroring vanilla's `SpreadContext.getCacheKey()` which subtracts the origin
/// before encoding. The previous implementation used raw world coordinates cast
/// to `i8`, which caused cache collisions at |X| or |Z| > 127.
///
/// # Performance
/// Reduces world lookups from ~20-30 to ~6 per spread calculation by caching
/// repeated accesses to the same positions during slope finding.
pub struct SpreadContext<'a> {
    /// Cache for block states by encoded relative position
    state_cache: FxHashMap<i16, BlockStateId>,
    /// Cache for hole check results by encoded relative position
    hole_cache: FxHashMap<i16, bool>,
    /// Reference to world for cache misses
    world: &'a World,
    /// The block from which spreading originates — used to compute relative cache keys.
    origin: BlockPos,
}

impl<'a> SpreadContext<'a> {
    /// Creates a new `SpreadContext` anchored at `origin`.
    ///
    /// `origin` must be the block that triggered the spread (the block passed to
    /// `get_spread()`), matching vanilla's `new FlowingFluid.SpreadContext(level, blockPos)`.
    #[must_use]
    pub fn new(world: &'a World, origin: BlockPos) -> Self {
        Self {
            state_cache: FxHashMap::default(),
            hole_cache: FxHashMap::default(),
            world,
            origin,
        }
    }

    /// Encodes a world position into a short cache key relative to the spread origin.
    ///
    /// Mirrors vanilla's `SpreadContext.getCacheKey(BlockPos)`:
    /// `(dx + 128 & 0xFF) << 8 | (dz + 128 & 0xFF)` where `dx`/`dz` are
    /// signed offsets from the origin, fitting within `i8` range.
    /// The previous implementation cast raw world coordinates to `i8`, which
    /// wrapped incorrectly at positions beyond ±127 absolute.
    fn encode_key(&self, pos: BlockPos) -> i16 {
        // Positions in the slope-finding algorithm stay within slopeFindDistance (<=4)
        // of the origin, so the difference always fits in i8.
        let dx = (pos.0.x - self.origin.0.x) as i8;
        let dz = (pos.0.z - self.origin.0.z) as i8;
        ((i16::from(dx) + 128) << 8) | (i16::from(dz) + 128)
    }

    /// Gets the cached block state at the given position, querying the world if not cached.
    #[must_use]
    pub fn get_block_state(&mut self, pos: BlockPos) -> BlockStateId {
        let key = self.encode_key(pos);
        *self
            .state_cache
            .entry(key)
            .or_insert_with(|| self.world.get_block_state(&pos))
    }

    /// Gets the cached block reference at the given position.
    #[must_use]
    pub fn get_block(&mut self, pos: BlockPos) -> BlockRef {
        self.get_block_state(pos).get_block()
    }

    /// Checks if the position is a hole (can fluid flow down into it?), with caching.
    #[must_use]
    pub fn is_hole(&mut self, pos: BlockPos, fluid_id: FluidRef) -> bool {
        let key = self.encode_key(pos);
        *self
            .hole_cache
            .entry(key)
            .or_insert_with(|| crate::fluid::is_hole(self.world, &pos, fluid_id))
    }

    /// Checks if fluid can pass horizontally to the given position.
    ///
    /// This uses the cached block state for efficiency.
    #[must_use]
    pub fn can_pass_horizontally(&mut self, pos: BlockPos, fluid_id: FluidRef) -> bool {
        let state = self.get_block_state(pos);
        can_pass_horizontally_internal(state, fluid_id)
    }

    /// Returns a reference to the world.
    #[must_use]
    pub fn world(&self) -> &'a World {
        self.world
    }
}
