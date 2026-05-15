use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{TreeBounds, TreePlacement};

const LEAF_DISTANCE_LIMIT: usize = 7;

impl FeatureDecorationRunner {
    pub(super) fn update_tree_leaves(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        bounds: TreeBounds,
        placement: &TreePlacement,
    ) {
        let mut shape = FxHashSet::default();
        for &pos in placement.decorations.iter().chain(placement.roots.iter()) {
            if bounds.contains(pos) {
                shape.insert(pos);
            }
        }

        let mut frontiers = (0..LEAF_DISTANCE_LIMIT)
            .map(|_| FxHashSet::default())
            .collect::<Vec<_>>();
        frontiers[0].extend(placement.trunks.iter().copied());
        let mut smallest_distance = 0;

        loop {
            while smallest_distance < LEAF_DISTANCE_LIMIT && frontiers[smallest_distance].is_empty()
            {
                smallest_distance += 1;
            }
            if smallest_distance >= LEAF_DISTANCE_LIMIT {
                break;
            }

            let Some(pos) = take_frontier_position(&mut frontiers[smallest_distance]) else {
                continue;
            };
            if !bounds.contains(pos) {
                continue;
            }

            if smallest_distance != 0 {
                let state = region.block_state(pos);
                if state
                    .try_get_value(&BlockStateProperties::DISTANCE)
                    .is_some()
                {
                    let distance = smallest_distance as u8;
                    Self::set_tree_block(
                        region,
                        pos,
                        state.set_value(&BlockStateProperties::DISTANCE, distance),
                    );
                }
            }

            shape.insert(pos);

            for direction in Self::VANILLA_DIRECTION_VALUES {
                let neighbor_pos = pos.relative(direction);
                if !bounds.contains(neighbor_pos) || shape.contains(&neighbor_pos) {
                    continue;
                }

                let state = region.block_state(neighbor_pos);
                let Some(distance) = Self::tree_optional_leaf_distance_at(registry, state) else {
                    continue;
                };
                let new_distance = distance.min((smallest_distance + 1) as u8);
                if new_distance < LEAF_DISTANCE_LIMIT as u8 {
                    frontiers[usize::from(new_distance)].insert(neighbor_pos);
                    smallest_distance = smallest_distance.min(usize::from(new_distance));
                }
            }
        }

        Self::update_tree_shape_at_edge(region, registry, &shape);
    }

    fn update_tree_shape_at_edge(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        shape: &FxHashSet<BlockPos>,
    ) {
        for &pos in shape {
            for direction in Self::VANILLA_DIRECTION_VALUES {
                let neighbor_pos = pos.relative(direction);
                if shape.contains(&neighbor_pos) {
                    continue;
                }

                let state = region.block_state(pos);
                let neighbor_state = region.block_state(neighbor_pos);
                Self::update_leaf_shape_at_edge(region, registry, pos, state, neighbor_state);
                Self::update_leaf_shape_at_edge(
                    region,
                    registry,
                    neighbor_pos,
                    neighbor_state,
                    state,
                );
            }
        }
    }

    fn update_leaf_shape_at_edge(
        region: &mut WorldGenRegion<'_>,
        registry: &Registry,
        pos: BlockPos,
        state: BlockStateId,
        neighbor_state: BlockStateId,
    ) {
        if !registry
            .blocks
            .is_in_tag(state.get_block(), &vanilla_block_tags::LEAVES_TAG)
        {
            return;
        }

        let Some(distance) = state.try_get_value(&BlockStateProperties::DISTANCE) else {
            return;
        };

        if !Self::tree_can_schedule_tick_at(region, pos) {
            return;
        }

        if state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true) {
            let _ = region.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, 5);
        }

        let distance_from_neighbor = Self::tree_leaf_distance_at(registry, neighbor_state) + 1;
        if distance_from_neighbor != 1 || distance != distance_from_neighbor {
            let _ = region.schedule_block_tick_default(pos, state.get_block(), 1);
        }
    }

    fn tree_optional_leaf_distance_at(registry: &Registry, state: BlockStateId) -> Option<u8> {
        if registry.blocks.is_in_tag(
            state.get_block(),
            &vanilla_block_tags::PREVENTS_NEARBY_LEAF_DECAY_TAG,
        ) {
            return Some(0);
        }

        state.try_get_value(&BlockStateProperties::DISTANCE)
    }

    fn tree_leaf_distance_at(registry: &Registry, state: BlockStateId) -> u8 {
        Self::tree_optional_leaf_distance_at(registry, state).unwrap_or(7)
    }

    fn tree_can_schedule_tick_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        region.can_write_to_chunk(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        )
    }
}

fn take_frontier_position(frontier: &mut FxHashSet<BlockPos>) -> Option<BlockPos> {
    let pos = frontier.iter().next().copied()?;
    frontier.remove(&pos);
    Some(pos)
}
