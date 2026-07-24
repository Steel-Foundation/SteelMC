//! Generation-only leaf-distance convergence.
//!
//! Vanilla lets tree edge-shape updates schedule leaf ticks, then converges the
//! affected leaf components after promotion through ordinary gameplay updates.
//! Steel resolves the current stable distances at generated Light instead. At
//! that point every Features task that can write the center chunk is complete,
//! the light work window provides a guarded 3x3 read cache, and only the center
//! proto chunk is mutated. A later outer-ring Features task can exceptionally
//! change the six-block halo inside a neighbor; generation accepts that rare
//! boundary staleness to keep this work off the game tick. Loaded chunks and
//! live leaf updates retain ordinary Vanilla tick behavior.

use std::{array, collections::VecDeque, sync::LazyLock};

use rustc_hash::{FxHashMap, FxHashSet};
use steel_registry::{
    REGISTRY,
    blocks::{
        BlockRef,
        properties::{BlockStateProperties, Property as _},
    },
    vanilla_block_tags::BlockTag,
};
use steel_utils::{BlockPos, BlockStateId, ChunkPos, Direction};

use crate::chunk::{
    chunk_access::{ChunkAccess, ChunkStatus},
    chunk_holder::ChunkHolder,
    light::LightWorkset,
};

const LEAF_DISTANCE_LIMIT: u8 = 7;
const MAX_PROPAGATED_DISTANCE: u8 = LEAF_DISTANCE_LIMIT - 1;
const LEAF_DISTANCES: [u8; LEAF_DISTANCE_LIMIT as usize] = [1, 2, 3, 4, 5, 6, 7];
const DIRECTIONS: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

#[derive(Clone, Copy)]
enum LeafDistanceStateInfo {
    Other,
    Source,
    Leaf {
        states_by_distance: [BlockStateId; LEAF_DISTANCE_LIMIT as usize],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LeafDistanceUpdate {
    pos: BlockPos,
    state: BlockStateId,
}

#[derive(Clone, Copy)]
struct LeafSolveBounds {
    center: ChunkPos,
    min_x: i32,
    max_x: i32,
    min_y: i32,
    max_y_exclusive: i32,
    min_z: i32,
    max_z: i32,
}

impl LeafSolveBounds {
    fn new(center: ChunkPos, min_y: i32, height: i32) -> Self {
        let center_min_x = center.0.x * 16;
        let center_min_z = center.0.y * 16;
        Self {
            center,
            min_x: center_min_x - i32::from(MAX_PROPAGATED_DISTANCE),
            max_x: center_min_x + 15 + i32::from(MAX_PROPAGATED_DISTANCE),
            min_y,
            max_y_exclusive: min_y + height,
            min_z: center_min_z - i32::from(MAX_PROPAGATED_DISTANCE),
            max_z: center_min_z + 15 + i32::from(MAX_PROPAGATED_DISTANCE),
        }
    }

    const fn contains(self, pos: BlockPos) -> bool {
        pos.x() >= self.min_x
            && pos.x() <= self.max_x
            && pos.y() >= self.min_y
            && pos.y() < self.max_y_exclusive
            && pos.z() >= self.min_z
            && pos.z() <= self.max_z
    }

    const fn is_center(self, pos: BlockPos) -> bool {
        ChunkPos::from_block_pos(pos).0.x == self.center.0.x
            && ChunkPos::from_block_pos(pos).0.y == self.center.0.y
    }
}

static LEAF_DISTANCE_STATE_INFO: LazyLock<Box<[LeafDistanceStateInfo]>> =
    LazyLock::new(build_leaf_distance_state_info);

pub(super) fn resolve_generated_leaf_distances(workset: &LightWorkset, holder: &ChunkHolder) {
    let pending_leaf_ticks = pending_leaf_tick_positions(holder);
    if pending_leaf_ticks.is_empty() {
        return;
    }

    let center = holder.get_pos();
    let min_y = holder.min_y();
    let height = holder.height();
    let updates = workset.with_chunk_read_cache(|chunk_cache| {
        chunk_cache.with_section_read_cache(|section_cache| {
            let layout = section_cache.layout();
            plan_leaf_distance_updates(center, min_y, height, &pending_leaf_ticks, |pos| {
                let Some(cached_block) = layout.cached_block(pos) else {
                    panic!("generated leaf-distance solver read {pos:?} outside its light cache");
                };
                section_cache.get_block_state(cached_block)
            })
        })
    });

    let Some(chunk) = holder.try_chunk(ChunkStatus::InitializeLight) else {
        panic!("center chunk disappeared during generated leaf-distance resolution");
    };
    let ChunkAccess::Proto(proto) = &*chunk else {
        panic!("generated leaf-distance resolution requires a proto chunk");
    };

    let mut writes = Vec::with_capacity(updates.len());
    for update in updates {
        let Ok(relative_x) = usize::try_from(update.pos.x().rem_euclid(16)) else {
            panic!("leaf write X coordinate cannot be represented as usize");
        };
        let Ok(relative_y) = usize::try_from(update.pos.y() - min_y) else {
            panic!("leaf write Y coordinate cannot be represented as usize");
        };
        let Ok(relative_z) = usize::try_from(update.pos.z().rem_euclid(16)) else {
            panic!("leaf write Z coordinate cannot be represented as usize");
        };
        writes.push((relative_x, relative_y, relative_z, update.state));
    }

    // Distance-only leaf changes preserve heightmap and light-source predicates.
    // The tracked batch still updates palette and random-tick section metadata,
    // while intentionally omitting neighbor-shape and observer callbacks so the
    // converged generation wave is not scheduled again.
    chunk.write_block_batch_for_generation(&writes);
    let removed_ticks = proto
        .block_ticks
        .lock()
        .remove_pending_matching(|tick| is_leaf_distance_block(tick.tick_type));
    if !writes.is_empty() || removed_ticks != 0 {
        chunk.mark_dirty();
    }
}

fn pending_leaf_tick_positions(holder: &ChunkHolder) -> Vec<BlockPos> {
    let Some(chunk) = holder.try_chunk(ChunkStatus::InitializeLight) else {
        panic!("generated leaf-distance resolution requires InitializeLight");
    };
    let ChunkAccess::Proto(proto) = &*chunk else {
        panic!("generated leaf-distance resolution requires a proto chunk");
    };
    let ticks = proto.block_ticks.lock();
    ticks
        .pending_entries()
        .iter()
        .filter(|tick| is_leaf_distance_block(tick.tick_type))
        .map(|tick| tick.pos)
        .collect()
}

fn plan_leaf_distance_updates(
    center: ChunkPos,
    min_y: i32,
    height: i32,
    seeds: &[BlockPos],
    mut state_at: impl FnMut(BlockPos) -> BlockStateId,
) -> Vec<LeafDistanceUpdate> {
    // A center leaf can only receive a final value below 7 from a source at
    // most six edges away, so the center chunk plus a six-block halo is enough.
    let bounds = LeafSolveBounds::new(center, min_y, height);
    let mut leaves = FxHashMap::default();
    let mut source_adjacent = FxHashSet::default();
    let mut component_queue = VecDeque::new();

    for &seed in seeds {
        if !bounds.is_center(seed) || !bounds.contains(seed) || leaves.contains_key(&seed) {
            continue;
        }

        let state = state_at(seed);
        if matches!(
            leaf_distance_state_info(state),
            LeafDistanceStateInfo::Leaf { .. }
        ) {
            leaves.insert(seed, state);
            component_queue.push_back(seed);
        }
    }

    while let Some(pos) = component_queue.pop_front() {
        for direction in DIRECTIONS {
            let neighbor = pos.relative(direction);
            if !bounds.contains(neighbor) || leaves.contains_key(&neighbor) {
                continue;
            }

            let state = state_at(neighbor);
            match leaf_distance_state_info(state) {
                LeafDistanceStateInfo::Source => {
                    source_adjacent.insert(pos);
                }
                LeafDistanceStateInfo::Leaf { .. } => {
                    leaves.insert(neighbor, state);
                    component_queue.push_back(neighbor);
                }
                LeafDistanceStateInfo::Other => {}
            }
        }
    }

    let mut distances = FxHashMap::default();
    let mut distance_queue = VecDeque::new();
    for pos in source_adjacent {
        distances.insert(pos, 1);
        distance_queue.push_back(pos);
    }

    while let Some(pos) = distance_queue.pop_front() {
        let Some(&distance) = distances.get(&pos) else {
            panic!("queued leaf position has no computed distance");
        };
        if distance >= MAX_PROPAGATED_DISTANCE {
            continue;
        }

        let next_distance = distance + 1;
        for direction in DIRECTIONS {
            let neighbor = pos.relative(direction);
            if leaves.contains_key(&neighbor) && !distances.contains_key(&neighbor) {
                distances.insert(neighbor, next_distance);
                distance_queue.push_back(neighbor);
            }
        }
    }

    let mut updates = Vec::new();
    for (pos, old_state) in leaves {
        if !bounds.is_center(pos) {
            continue;
        }

        let distance = distances.get(&pos).copied().unwrap_or(LEAF_DISTANCE_LIMIT);
        let LeafDistanceStateInfo::Leaf { states_by_distance } =
            leaf_distance_state_info(old_state)
        else {
            panic!("leaf component contains a non-leaf state");
        };
        let new_state = states_by_distance[usize::from(distance - 1)];
        if new_state != old_state {
            updates.push(LeafDistanceUpdate {
                pos,
                state: new_state,
            });
        }
    }

    updates.sort_unstable_by_key(|update| {
        (
            (update.pos.y() - min_y) / 16,
            update.pos.y(),
            update.pos.z(),
            update.pos.x(),
        )
    });
    updates
}

fn leaf_distance_state_info(state: BlockStateId) -> LeafDistanceStateInfo {
    let Some(&info) = LEAF_DISTANCE_STATE_INFO.get(state.0 as usize) else {
        panic!("invalid block state id {} in leaf-distance solver", state.0);
    };
    info
}

fn is_leaf_distance_block(block: BlockRef) -> bool {
    matches!(
        leaf_distance_state_info(block.default_state()),
        LeafDistanceStateInfo::Leaf { .. }
    )
}

fn build_leaf_distance_state_info() -> Box<[LeafDistanceStateInfo]> {
    let mut state_info =
        vec![LeafDistanceStateInfo::Other; REGISTRY.blocks.state_to_block_lookup.len()];

    for (block_id, block) in REGISTRY.blocks.iter() {
        let base_state = REGISTRY.blocks.block_to_base_state[block_id];
        let state_count = block.state_count();
        if block.has_tag(&BlockTag::PREVENTS_NEARBY_LEAF_DECAY) {
            for offset in 0..state_count {
                state_info[usize::from(base_state + offset)] = LeafDistanceStateInfo::Source;
            }
            continue;
        }

        if !block.has_tag(&BlockTag::LEAVES) || !has_leaf_distance_property(block) {
            continue;
        }

        for offset in 0..state_count {
            let state = BlockStateId(base_state + offset);
            let states_by_distance = array::from_fn(|index| {
                REGISTRY.blocks.set_property(
                    state,
                    &BlockStateProperties::DISTANCE,
                    LEAF_DISTANCES[index],
                )
            });
            state_info[usize::from(state.0)] = LeafDistanceStateInfo::Leaf { states_by_distance };
        }
    }

    state_info.into_boxed_slice()
}

fn has_leaf_distance_property(block: BlockRef) -> bool {
    let expected = &BlockStateProperties::DISTANCE;
    // Match the complete 1..=7 descriptor, not just the serialized name:
    // scaffolding owns a distinct 0..=7 property also named `distance`.
    block.properties.iter().any(|property| {
        property.get_name() == expected.name
            && property.value_count() == expected.value_count()
            && (0..expected.value_count()).all(|index| {
                property.value_name_from_index(index) == expected.value_name_from_index(index)
            })
    })
}

#[cfg(test)]
mod tests {
    use steel_registry::{
        blocks::block_state_ext::BlockStateExt as _, test_support::init_test_registry,
        vanilla_blocks,
    };

    use super::*;

    fn map_state_reader(
        states: &FxHashMap<BlockPos, BlockStateId>,
    ) -> impl FnMut(BlockPos) -> BlockStateId + '_ {
        let air = vanilla_blocks::AIR.default_state();
        move |pos| states.get(&pos).copied().unwrap_or(air)
    }

    #[test]
    fn resolves_center_leaves_from_sources_across_chunk_boundaries() {
        init_test_registry();
        let center = ChunkPos::new(0, 0);
        let center_leaf = BlockPos::new(15, 8, 8);
        let neighbor_leaf = BlockPos::new(16, 8, 8);
        let source = BlockPos::new(17, 8, 8);
        let distance_seven = vanilla_blocks::OAK_LEAVES
            .default_state()
            .set_value(&BlockStateProperties::DISTANCE, 7)
            .set_value(&BlockStateProperties::PERSISTENT, true)
            .set_value(&BlockStateProperties::WATERLOGGED, true);
        let mut states = FxHashMap::default();
        states.insert(center_leaf, distance_seven);
        states.insert(neighbor_leaf, distance_seven);
        states.insert(source, vanilla_blocks::OAK_LOG.default_state());

        let updates =
            plan_leaf_distance_updates(center, 0, 16, &[center_leaf], map_state_reader(&states));

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].pos, center_leaf);
        assert_eq!(
            updates[0]
                .state
                .try_get_value(&BlockStateProperties::DISTANCE),
            Some(2)
        );
        assert_eq!(
            updates[0]
                .state
                .try_get_value(&BlockStateProperties::PERSISTENT),
            Some(true)
        );
        assert_eq!(
            updates[0]
                .state
                .try_get_value(&BlockStateProperties::WATERLOGGED),
            Some(true)
        );
        assert!(updates.iter().all(|update| update.pos != neighbor_leaf));
    }

    #[test]
    fn stale_leaf_distances_without_a_source_converge_to_seven() {
        init_test_registry();
        let first = BlockPos::new(8, 8, 8);
        let second = BlockPos::new(9, 8, 8);
        let leaves = vanilla_blocks::OAK_LEAVES.default_state();
        let unrelated = BlockPos::new(12, 8, 8);
        let mut states = FxHashMap::default();
        states.insert(first, leaves.set_value(&BlockStateProperties::DISTANCE, 1));
        states.insert(second, leaves.set_value(&BlockStateProperties::DISTANCE, 2));
        states.insert(
            unrelated,
            leaves.set_value(&BlockStateProperties::DISTANCE, 1),
        );

        let updates = plan_leaf_distance_updates(
            ChunkPos::new(0, 0),
            0,
            16,
            &[first],
            map_state_reader(&states),
        );

        assert_eq!(updates.len(), 2);
        assert!(updates.iter().all(|update| {
            update.state.try_get_value(&BlockStateProperties::DISTANCE) == Some(7)
        }));
        assert!(updates.iter().all(|update| update.pos != unrelated));
    }

    #[test]
    fn six_block_halo_preserves_the_distance_seven_boundary() {
        init_test_registry();
        let seed = BlockPos::new(15, 8, 8);
        let leaves = vanilla_blocks::OAK_LEAVES.default_state();
        let mut distance_six_states = FxHashMap::default();
        for x in 15..=20 {
            let current_distance = if x == 15 { 1 } else { 7 };
            distance_six_states.insert(
                BlockPos::new(x, 8, 8),
                leaves.set_value(&BlockStateProperties::DISTANCE, current_distance),
            );
        }
        distance_six_states.insert(
            BlockPos::new(21, 8, 8),
            vanilla_blocks::OAK_LOG.default_state(),
        );

        let distance_six = plan_leaf_distance_updates(
            ChunkPos::new(0, 0),
            0,
            16,
            &[seed],
            map_state_reader(&distance_six_states),
        );
        assert_eq!(
            distance_six[0]
                .state
                .try_get_value(&BlockStateProperties::DISTANCE),
            Some(6)
        );

        let mut distance_seven_states = distance_six_states;
        distance_seven_states.insert(
            BlockPos::new(21, 8, 8),
            leaves.set_value(&BlockStateProperties::DISTANCE, 6),
        );
        distance_seven_states.insert(
            BlockPos::new(22, 8, 8),
            vanilla_blocks::OAK_LOG.default_state(),
        );
        let distance_seven = plan_leaf_distance_updates(
            ChunkPos::new(0, 0),
            0,
            16,
            &[seed],
            map_state_reader(&distance_seven_states),
        );
        assert_eq!(
            distance_seven[0]
                .state
                .try_get_value(&BlockStateProperties::DISTANCE),
            Some(7)
        );
    }

    #[test]
    fn scaffolding_distance_is_not_leaf_distance() {
        init_test_registry();
        let scaffolding = vanilla_blocks::SCAFFOLDING
            .default_state()
            .set_value(&BlockStateProperties::STABILITY_DISTANCE, 1);

        assert!(matches!(
            leaf_distance_state_info(scaffolding),
            LeafDistanceStateInfo::Other
        ));
    }
}
