//! Fluid collision and passability logic.
//!
//! Equivalent to various collision checks in FlowingFluid.java.

use std::cell::RefCell;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_registry::blocks::shapes::ShapeChannel;
use steel_registry::fluid::FluidRef;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::{BLOCK_BEHAVIORS, FLUID_BEHAVIORS};
use crate::physics::shapes::merged_offset_face_occludes;
use crate::world::World;

const OCCLUSION_CACHE_CAPACITY: usize = 200;
const NO_CACHE_ENTRY: u16 = u16::MAX;

/// Blocks whose collision shape depends on position bypass the cache, so the
/// state pair and direction fully identify the result.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BlockStatePairKey {
    from: BlockStateId,
    to: BlockStateId,
    direction: Direction,
}

#[derive(Clone, Copy)]
struct OcclusionCacheEntry {
    key: BlockStatePairKey,
    passes: bool,
    prev: u16,
    next: u16,
}

struct OcclusionCache {
    entries: Vec<OcclusionCacheEntry>,
    lookup: FxHashMap<BlockStatePairKey, u16>,
    head: u16,
    tail: u16,
    free: u16,
}

impl OcclusionCache {
    fn new() -> Self {
        let mut entries = Vec::with_capacity(OCCLUSION_CACHE_CAPACITY);
        for index in 0..OCCLUSION_CACHE_CAPACITY {
            let next = if index + 1 < OCCLUSION_CACHE_CAPACITY {
                (index + 1) as u16
            } else {
                NO_CACHE_ENTRY
            };
            entries.push(OcclusionCacheEntry {
                key: BlockStatePairKey {
                    from: BlockStateId(0),
                    to: BlockStateId(0),
                    direction: Direction::Down,
                },
                passes: false,
                prev: NO_CACHE_ENTRY,
                next,
            });
        }

        Self {
            entries,
            lookup: FxHashMap::default(),
            head: NO_CACHE_ENTRY,
            tail: NO_CACHE_ENTRY,
            free: 0,
        }
    }

    fn get(&mut self, key: BlockStatePairKey) -> Option<bool> {
        let &index = self.lookup.get(&key)?;
        self.move_to_front(index);
        Some(self.entries[index as usize].passes)
    }

    fn insert(&mut self, key: BlockStatePairKey, passes: bool) {
        let index = if self.free == NO_CACHE_ENTRY {
            let index = self.tail;
            let evicted = self.entries[index as usize].key;
            self.unlink(index);
            self.lookup.remove(&evicted);
            index
        } else {
            let index = self.free;
            self.free = self.entries[index as usize].next;
            index
        };

        self.entries[index as usize] = OcclusionCacheEntry {
            key,
            passes,
            prev: NO_CACHE_ENTRY,
            next: self.head,
        };
        if self.head != NO_CACHE_ENTRY {
            self.entries[self.head as usize].prev = index;
        }
        self.head = index;
        if self.tail == NO_CACHE_ENTRY {
            self.tail = index;
        }
        self.lookup.insert(key, index);
    }

    fn move_to_front(&mut self, index: u16) {
        if self.head == index {
            return;
        }

        self.unlink(index);
        self.entries[index as usize].prev = NO_CACHE_ENTRY;
        self.entries[index as usize].next = self.head;
        if self.head != NO_CACHE_ENTRY {
            self.entries[self.head as usize].prev = index;
        }
        self.head = index;
        if self.tail == NO_CACHE_ENTRY {
            self.tail = index;
        }
    }

    fn unlink(&mut self, index: u16) {
        let entry = self.entries[index as usize];
        if entry.prev == NO_CACHE_ENTRY {
            self.head = entry.next;
        } else {
            self.entries[entry.prev as usize].next = entry.next;
        }
        if entry.next == NO_CACHE_ENTRY {
            self.tail = entry.prev;
        } else {
            self.entries[entry.next as usize].prev = entry.prev;
        }
    }
}

thread_local! {
    static OCCLUSION_CACHE: RefCell<OcclusionCache> = RefCell::new(OcclusionCache::new());
}

/// Vanilla bypasses the cache for `hasDynamicShape()` blocks. Steel also
/// bypasses any block whose collision shape uses a positional offset, even if
/// its block config is not marked dynamic.
fn has_position_dependent_collision_shape(state: BlockStateId) -> bool {
    let block = state.get_block();
    block.config.dynamic_shape || block.shape_offsets.uses_offset(ShapeChannel::Collision)
}

#[cfg(any(test, feature = "benchmark-support"))]
fn compute_can_pass_through_wall(
    from: BlockPos,
    from_state: BlockStateId,
    to: BlockPos,
    to_state: BlockStateId,
    direction: Direction,
) -> bool {
    let from_shape = from_state.get_collision_shape_at(from);
    let to_shape = to_state.get_collision_shape_at(to);

    !merged_offset_face_occludes(from_shape, to_shape, direction)
}

/// Checks if fluid can pass through a wall between two positions.
#[must_use]
pub fn can_pass_through_wall(
    world: &Arc<World>,
    from: BlockPos,
    from_state: BlockStateId,
    to: BlockPos,
    to_state: BlockStateId,
    direction: Direction,
) -> bool {
    if !world.is_in_valid_bounds(to) {
        return false;
    }

    let cacheable = !has_position_dependent_collision_shape(from_state)
        && !has_position_dependent_collision_shape(to_state);
    let key = BlockStatePairKey {
        from: from_state,
        to: to_state,
        direction,
    };

    if cacheable && let Some(passes) = OCCLUSION_CACHE.with_borrow_mut(|cache| cache.get(key)) {
        return passes;
    }

    let from_shape = from_state.get_collision_shape_at(from);
    let to_shape = to_state.get_collision_shape_at(to);
    let passes = !merged_offset_face_occludes(from_shape, to_shape, direction);

    if cacheable {
        OCCLUSION_CACHE.with_borrow_mut(|cache| cache.insert(key, passes));
    }

    passes
}

#[cfg(feature = "benchmark-support")]
#[doc(hidden)]
#[must_use]
pub fn can_pass_through_wall_uncached(
    world: &Arc<World>,
    from: BlockPos,
    from_state: BlockStateId,
    to: BlockPos,
    to_state: BlockStateId,
    direction: Direction,
) -> bool {
    if !world.is_in_valid_bounds(to) {
        return false;
    }

    compute_can_pass_through_wall(from, from_state, to, to_state, direction)
}

/// Checks if a block state can hold any fluid, without world access.
///
/// Vanilla equivalent: `FlowingFluid.canHoldAnyFluid(BlockState)`.
/// Uses `blocksMotion()` check instead of `has_collision` - vanilla's
/// `blocksMotion()` = `block != Cobweb && block != BambooSapling && isSolid()`.
#[must_use]
pub fn can_hold_any_fluid_state(state: BlockStateId) -> bool {
    let block = state.get_block();

    // Vanilla: block instanceof LiquidBlockContainer → true
    if state.is_liquid_container() {
        return true;
    }

    // Vanilla: state.blocksMotion() ? false : !(exclusion list)
    if state.blocks_motion() {
        return false;
    }

    // Non-solid blocks that still reject fluid.
    !is_fluid_excluded_block(block)
}

/// Returns true if a block is in the vanilla fluid exclusion list.
fn is_fluid_excluded_block(block: BlockRef) -> bool {
    block == &vanilla_blocks::LADDER
        || block == &vanilla_blocks::SUGAR_CANE
        || block == &vanilla_blocks::BUBBLE_COLUMN
        || block == &vanilla_blocks::NETHER_PORTAL
        || block == &vanilla_blocks::END_PORTAL
        || block == &vanilla_blocks::END_GATEWAY
        || block == &vanilla_blocks::STRUCTURE_VOID
        || block.has_tag(&BlockTag::SIGNS)
        || block.has_tag(&BlockTag::DOORS)
}

/// Vanilla equivalent: `FlowingFluid.canHoldSpecificFluid(BlockGetter, BlockPos, BlockState, Fluid)`.
///
/// For `LiquidBlockContainer` blocks, delegates to `canPlaceLiquid(null, ...)`.
/// For other blocks, always returns true.
#[must_use]
pub fn can_hold_specific_fluid(state: BlockStateId, fluid: FluidRef) -> bool {
    if state.is_liquid_container() {
        let behavior = BLOCK_BEHAVIORS.get_behavior(state.get_block());
        behavior.can_place_liquid(state, fluid)
    } else {
        true
    }
}

/// Vanilla equivalent: `FlowingFluid.canHoldFluid(BlockGetter, BlockPos, BlockState, Fluid)`.
///
/// Combined check: `canHoldAnyFluid(state) && canHoldSpecificFluid(state, fluid)`.
#[must_use]
pub fn can_hold_fluid(state: BlockStateId, fluid: FluidRef) -> bool {
    can_hold_any_fluid_state(state) && can_hold_specific_fluid(state, fluid)
}

/// Core passability logic for horizontal fluid spread.
///
/// Vanilla equivalent: `!isSourceBlockOfThisType(testFluidState) && canHoldAnyFluid(testState)`.
#[must_use]
pub fn can_pass_horizontally_internal(state: BlockStateId, target_fluid_id: FluidRef) -> bool {
    // Vanilla: !isSourceBlockOfThisType — reject same-type source blocks
    let fluid_state = state.get_fluid_state();
    if FLUID_BEHAVIORS
        .get_behavior(target_fluid_id)
        .is_same(fluid_state.fluid_id)
        && fluid_state.is_source()
    {
        return false;
    }

    // Vanilla: canHoldAnyFluid
    can_hold_any_fluid_state(state)
}

#[cfg(test)]
mod tests {
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};
    use steel_registry::blocks::properties::{BlockStateProperties, SlabType};
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_fluids};
    use steel_utils::ChunkPos;

    use super::*;

    fn states_with_non_trivial_shapes() -> Vec<BlockStateId> {
        vec![
            vanilla_blocks::AIR.default_state(),
            vanilla_blocks::STONE.default_state(),
            vanilla_blocks::WATER.default_state(),
            vanilla_blocks::OAK_SLAB
                .default_state()
                .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom),
            vanilla_blocks::OAK_STAIRS.default_state(),
            vanilla_blocks::OAK_FENCE.default_state(),
            vanilla_blocks::BAMBOO.default_state(),
            vanilla_blocks::POINTED_DRIPSTONE.default_state(),
            vanilla_blocks::SULFUR_SPIKE.default_state(),
        ]
    }

    #[test]
    fn waterlogged_source_blocks_block_same_fluid_flowing_variant_spread() {
        init_vanilla_registry();
        init_behaviors();

        let waterlogged = vanilla_blocks::OAK_LEAVES
            .default_state()
            .set_value(&BlockStateProperties::WATERLOGGED, true);

        assert!(!can_pass_horizontally_internal(
            waterlogged,
            &vanilla_fluids::FLOWING_WATER
        ));
        assert!(!can_pass_horizontally_internal(
            waterlogged,
            &vanilla_fluids::WATER
        ));
    }

    #[test]
    fn dry_waterloggable_blocks_reject_flowing_water() {
        init_vanilla_registry();
        init_behaviors();

        let dry = vanilla_blocks::OAK_SLAB
            .default_state()
            .set_value(&BlockStateProperties::SLAB_TYPE, SlabType::Bottom)
            .set_value(&BlockStateProperties::WATERLOGGED, false);

        assert!(can_hold_specific_fluid(dry, &vanilla_fluids::WATER));
        assert!(!can_hold_specific_fluid(
            dry,
            &vanilla_fluids::FLOWING_WATER
        ));
    }

    #[test]
    fn kelp_is_liquid_container_without_waterlogged_property() {
        init_vanilla_registry();
        init_behaviors();

        let kelp = vanilla_blocks::KELP.default_state();

        assert!(
            kelp.try_get_value(&BlockStateProperties::WATERLOGGED)
                .is_none()
        );
        assert!(kelp.is_liquid_container());
        assert!(can_hold_any_fluid_state(kelp));
        assert!(!can_hold_specific_fluid(kelp, &vanilla_fluids::WATER));
        assert!(!can_hold_specific_fluid(kelp, &vanilla_fluids::LAVA));
    }

    #[test]
    fn cached_occlusion_matches_the_uncached_result() {
        init_vanilla_registry();
        init_behaviors();

        let fixture = fresh_test_world("fluid_occlusion_cache_equivalence");
        let world = &fixture.world;
        insert_ready_full_chunk(world, ChunkPos::new(0, 0));

        let states = states_with_non_trivial_shapes();
        let positions = [BlockPos::new(8, 64, 8), BlockPos::new(9, 64, 9)];
        let position_pairs = [
            (positions[0], positions[0]),
            (positions[1], positions[1]),
            (positions[0], positions[1]),
            (positions[1], positions[0]),
        ];
        let directions = [
            Direction::Down,
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::West,
            Direction::East,
        ];

        for &from_state in &states {
            for &to_state in &states {
                // Reuse the same state pair at several positions so a cache
                // entry that ignored a positional shape offset would fail.
                for (from, to) in position_pairs {
                    for direction in directions {
                        let expected = compute_can_pass_through_wall(
                            from, from_state, to, to_state, direction,
                        );
                        let cached =
                            can_pass_through_wall(world, from, from_state, to, to_state, direction);
                        assert_eq!(
                            cached, expected,
                            "from_pos={:?} to_pos={:?} from={from_state:?} to={to_state:?} direction={direction:?}",
                            from.0, to.0
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn position_dependent_collision_shapes_are_not_cacheable() {
        init_vanilla_registry();

        // Caching any of these would make the result depend on whichever
        // position populated the entry.
        for state in [
            vanilla_blocks::MOVING_PISTON.default_state(),
            vanilla_blocks::BAMBOO.default_state(),
            vanilla_blocks::SCAFFOLDING.default_state(),
            vanilla_blocks::POWDER_SNOW.default_state(),
            vanilla_blocks::POINTED_DRIPSTONE.default_state(),
            vanilla_blocks::SULFUR_SPIKE.default_state(),
            vanilla_blocks::SHULKER_BOX.default_state(),
        ] {
            assert!(
                has_position_dependent_collision_shape(state),
                "{state:?} must bypass the occlusion cache"
            );
        }

        for state in [
            vanilla_blocks::AIR.default_state(),
            vanilla_blocks::STONE.default_state(),
            vanilla_blocks::OAK_SLAB.default_state(),
            vanilla_blocks::OAK_STAIRS.default_state(),
            vanilla_blocks::OAK_FENCE.default_state(),
        ] {
            assert!(
                !has_position_dependent_collision_shape(state),
                "{state:?} has a state-only collision shape and should use the cache"
            );
        }
    }

    #[test]
    fn cache_evicts_the_least_recently_used_entry() {
        let mut cache = OcclusionCache::new();
        let key = |from: u16, to: u16, direction| BlockStatePairKey {
            from: BlockStateId(from),
            to: BlockStateId(to),
            direction,
        };

        cache.insert(key(1, 2, Direction::North), true);
        for index in 0..OCCLUSION_CACHE_CAPACITY - 1 {
            cache.insert(key(100 + index as u16, 200, Direction::South), false);
        }
        assert_eq!(cache.get(key(1, 2, Direction::North)), Some(true));

        // Touch the two oldest entries, then add one more: the next oldest
        // entry (101, 200) is evicted.
        assert_eq!(cache.get(key(100, 200, Direction::South)), Some(false));
        cache.insert(key(500, 600, Direction::East), true);

        assert_eq!(cache.get(key(1, 2, Direction::North)), Some(true));
        assert_eq!(cache.get(key(100, 200, Direction::South)), Some(false));
        assert_eq!(cache.get(key(101, 200, Direction::South)), None);
    }
}
