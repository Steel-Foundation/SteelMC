use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::{TreeBounds, TreePlacement};

const LEAF_DISTANCE_LIMIT: usize = 7;

impl FeatureDecorationRunner {
    pub(super) fn update_tree_leaves(
        region: &mut WorldGenRegion<'_>,
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
                let Some(distance) = state.try_get_value(&BlockStateProperties::DISTANCE) else {
                    continue;
                };
                let new_distance = distance.min((smallest_distance + 1) as u8);
                if new_distance < LEAF_DISTANCE_LIMIT as u8 {
                    frontiers[usize::from(new_distance)].insert(neighbor_pos);
                    smallest_distance = smallest_distance.min(usize::from(new_distance));
                }
            }
        }

        // TODO: Mirror `StructureTemplate.updateShapeAtEdge` once block behaviors expose
        // vanilla shape update hooks for generation-time leaf edges.
    }
}

fn take_frontier_position(frontier: &mut FxHashSet<BlockPos>) -> Option<BlockPos> {
    let pos = frontier.iter().next().copied()?;
    frontier.remove(&pos);
    Some(pos)
}
