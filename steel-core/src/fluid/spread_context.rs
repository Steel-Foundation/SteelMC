//! Spread calculation context for fluid flow optimization.
//!
//! Based on vanilla's FlowingFluid.SpreadContext, this provides local caching
//! of block states and hole checks during fluid spread calculations.
//!
//! This avoids repeatedly querying the world for the same positions during
//! the recursive slope-finding algorithm.

use std::sync::Arc;

use steel_registry::fluid::FluidRef;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::fluid::is_hole;
use crate::world::World;

/// Furthest horizontal offset visited by `getSpread` and `getSlopeDistance`.
///
/// `getSpread` checks the four direct neighbors, and each slope recursion can
/// advance one more block up to `slopeFindDistance`. Vanilla caps that distance
/// at 4, so the outermost checked position is 5 blocks from the origin.
const CACHE_RADIUS: i32 = 5;
const CACHE_DIAMETER: usize = (CACHE_RADIUS * 2 + 1) as usize;
const CACHE_LEN: usize = CACHE_DIAMETER * CACHE_DIAMETER;

/// Context for fluid spread calculations with local caching.
///
/// Created fresh for each `get_spread()` call. Cache keys ignore Y: every
/// position visited by a single spread calculation lies on the origin's Y level.
pub(super) struct SpreadContext<'a> {
    state_cache: [Option<BlockStateId>; CACHE_LEN],
    hole_cache: [Option<bool>; CACHE_LEN],
    world: &'a Arc<World>,
    origin: BlockPos,
}

impl<'a> SpreadContext<'a> {
    /// Creates a new `SpreadContext` anchored at `origin`.
    ///
    /// `origin` must be the block that triggered the spread (the block passed to
    /// `get_spread()`), matching vanilla's `new FlowingFluid.SpreadContext(level, blockPos)`.
    #[must_use]
    pub(super) const fn new(world: &'a Arc<World>, origin: BlockPos) -> Self {
        Self {
            state_cache: [None; CACHE_LEN],
            hole_cache: [None; CACHE_LEN],
            world,
            origin,
        }
    }

    fn cache_index(&self, pos: BlockPos) -> Option<usize> {
        let dx = pos.0.x - self.origin.0.x;
        let dz = pos.0.z - self.origin.0.z;
        if !(-CACHE_RADIUS..=CACHE_RADIUS).contains(&dx)
            || !(-CACHE_RADIUS..=CACHE_RADIUS).contains(&dz)
        {
            return None;
        }

        let x = (dx + CACHE_RADIUS) as usize;
        let z = (dz + CACHE_RADIUS) as usize;
        Some(x * CACHE_DIAMETER + z)
    }

    #[must_use]
    pub fn get_block_state(&mut self, pos: BlockPos) -> BlockStateId {
        let Some(index) = self.cache_index(pos) else {
            return self.world.get_block_state(pos);
        };

        if let Some(state) = self.state_cache[index] {
            return state;
        }

        let state = self.world.get_block_state(pos);
        self.state_cache[index] = Some(state);
        state
    }

    /// Seeds a state already read by the outer spread calculation.
    pub fn cache_block_state(&mut self, pos: BlockPos, state: BlockStateId) {
        if let Some(index) = self.cache_index(pos) {
            self.state_cache[index] = Some(state);
        }
    }

    /// Checks if the position is a hole (can fluid flow down into it?), with caching.
    #[must_use]
    pub fn is_hole(&mut self, pos: BlockPos, fluid_id: FluidRef) -> bool {
        let Some(index) = self.cache_index(pos) else {
            let state = self.get_block_state(pos);
            let below = pos.below();
            let below_state = self.world.get_block_state(below);
            return is_hole(self.world, pos, state, below, below_state, fluid_id);
        };

        if let Some(is_hole) = self.hole_cache[index] {
            return is_hole;
        }

        let state = self.get_block_state(pos);
        let below = pos.below();
        let below_state = self.world.get_block_state(below);
        let result = is_hole(self.world, pos, state, below, below_state, fluid_id);
        self.hole_cache[index] = Some(result);
        result
    }

    #[must_use]
    pub(super) const fn world(&self) -> &'a Arc<World> {
        self.world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{TestWorld, fresh_test_world, insert_ready_full_chunk};
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_fluids};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    fn setup_world(key: &'static str) -> (TestWorld, BlockPos) {
        init_vanilla_registry();
        init_behaviors();

        let fixture = fresh_test_world(key);
        insert_ready_full_chunk(&fixture.world, ChunkPos::new(0, 0));
        (fixture, BlockPos::new(8, 64, 8))
    }

    #[test]
    fn seeded_states_are_returned_without_world_reads() {
        let (fixture, origin) = setup_world("fluid_spread_context_seed");
        let stone = vanilla_blocks::STONE.default_state();
        let mut context = SpreadContext::new(&fixture.world, origin);

        context.cache_block_state(origin, stone);
        assert_eq!(context.get_block_state(origin), stone);

        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        assert!(fixture.world.set_block_with_limit(
            origin,
            vanilla_blocks::DIRT.default_state(),
            flags,
            0
        ));
        assert_eq!(context.get_block_state(origin), stone);
    }

    #[test]
    fn cache_window_boundaries_fall_back_to_the_world() {
        let (fixture, origin) = setup_world("fluid_spread_context_fallback");
        let stone = vanilla_blocks::STONE.default_state();
        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        let outside = origin.offset(6, 0, 0);
        assert!(fixture.world.set_block_with_limit(outside, stone, flags, 0));

        let mut context = SpreadContext::new(&fixture.world, origin);
        assert!(context.cache_index(origin.offset(-5, 0, -5)).is_some());
        assert!(context.cache_index(origin.offset(5, 0, 5)).is_some());
        assert!(context.cache_index(outside).is_none());
        assert!(context.cache_index(origin.offset(0, 0, -6)).is_none());
        // Y is intentionally ignored, matching vanilla's short cache key.
        assert_eq!(
            context.cache_index(origin.offset(0, 1, 0)),
            context.cache_index(origin)
        );

        assert_eq!(context.get_block_state(outside), stone);
        assert!(!context.is_hole(outside, &vanilla_fluids::WATER));
    }

    #[test]
    fn hole_results_are_cached_per_relative_position() {
        let (fixture, origin) = setup_world("fluid_spread_context_hole_cache");
        let flags = UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE;
        assert!(fixture.world.set_block_with_limit(
            origin,
            vanilla_blocks::WATER.default_state(),
            flags,
            0
        ));

        let mut context = SpreadContext::new(&fixture.world, origin);
        let first = context.is_hole(origin, &vanilla_fluids::WATER);
        assert!(first);

        // Mutating the world after the first lookup must not change the cached result.
        assert!(fixture.world.set_block_with_limit(
            origin.below(),
            vanilla_blocks::STONE.default_state(),
            flags,
            0
        ));
        assert_eq!(context.is_hole(origin, &vanilla_fluids::WATER), first);
    }
}
