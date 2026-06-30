use steel_registry::vanilla_block_tags::BlockTag;

use super::super::super::prelude::*;
use super::super::super::runner::FeatureDecorationRunner;
use super::super::super::vanilla_collections::JavaBlockPosSet;
use super::{TreeBounds, TreePlacement};

const LEAF_DISTANCE_LIMIT: usize = 7;

impl FeatureDecorationRunner {
    pub(super) fn update_tree_leaves(
        region: &mut WorldGenRegion<'_>,
        bounds: TreeBounds,
        placement: &TreePlacement,
    ) {
        let mut shape = FxHashSet::default();
        for pos in placement
            .decorations
            .java_ordered_positions()
            .into_iter()
            .chain(placement.roots.java_ordered_positions())
        {
            if bounds.contains(pos) {
                shape.insert(pos);
            }
        }

        let mut frontiers = (0..LEAF_DISTANCE_LIMIT)
            .map(|_| JavaBlockPosSet::default())
            .collect::<Vec<_>>();
        for pos in placement.trunks.java_ordered_positions() {
            frontiers[0].insert(pos);
        }
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
                let Some(distance) = Self::tree_optional_leaf_distance_at(state) else {
                    continue;
                };
                let new_distance = distance.min((smallest_distance + 1) as u8);
                if new_distance < LEAF_DISTANCE_LIMIT as u8 {
                    frontiers[usize::from(new_distance)].insert(neighbor_pos);
                    smallest_distance = smallest_distance.min(usize::from(new_distance));
                }
            }
        }

        Self::update_tree_shape_at_edge(region, bounds, &shape);
    }

    fn update_tree_shape_at_edge(
        region: &WorldGenRegion<'_>,
        bounds: TreeBounds,
        shape: &FxHashSet<BlockPos>,
    ) {
        for x in bounds.min_x..=bounds.max_x {
            for y in bounds.min_y..=bounds.max_y {
                Self::scan_tree_shape_line(
                    region,
                    shape,
                    bounds.min_z,
                    bounds.max_z,
                    |z| BlockPos::new(x, y, z),
                    Direction::North,
                    Direction::South,
                );
            }
        }

        for z in bounds.min_z..=bounds.max_z {
            for x in bounds.min_x..=bounds.max_x {
                Self::scan_tree_shape_line(
                    region,
                    shape,
                    bounds.min_y,
                    bounds.max_y,
                    |y| BlockPos::new(x, y, z),
                    Direction::Down,
                    Direction::Up,
                );
            }
        }

        for y in bounds.min_y..=bounds.max_y {
            for z in bounds.min_z..=bounds.max_z {
                Self::scan_tree_shape_line(
                    region,
                    shape,
                    bounds.min_x,
                    bounds.max_x,
                    |x| BlockPos::new(x, y, z),
                    Direction::West,
                    Direction::East,
                );
            }
        }
    }

    fn scan_tree_shape_line(
        region: &WorldGenRegion<'_>,
        shape: &FxHashSet<BlockPos>,
        start: i32,
        end: i32,
        mut pos_at: impl FnMut(i32) -> BlockPos,
        negative: Direction,
        positive: Direction,
    ) {
        let mut last_full = false;
        for cursor in start..=end + 1 {
            let full = cursor != end + 1 && shape.contains(&pos_at(cursor));
            if !last_full && full {
                Self::update_tree_shape_face(region, pos_at(cursor), negative);
            }

            if last_full && !full {
                Self::update_tree_shape_face(region, pos_at(cursor - 1), positive);
            }

            last_full = full;
        }
    }

    fn update_tree_shape_face(region: &WorldGenRegion<'_>, pos: BlockPos, direction: Direction) {
        let neighbor_pos = pos.relative(direction);
        let state = region.block_state(pos);
        let neighbor_state = region.block_state(neighbor_pos);

        Self::update_leaf_shape_at_edge(region, pos, state, neighbor_state);
        Self::update_leaf_shape_at_edge(region, neighbor_pos, neighbor_state, state);

        let new_state = BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .update_shape(state, region, pos, direction, neighbor_pos, neighbor_state);
        if state != new_state {
            let _ = region.set_block_state(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }

        let new_neighbor_state = BLOCK_BEHAVIORS
            .get_behavior(neighbor_state.get_block())
            .update_shape(
                neighbor_state,
                region,
                neighbor_pos,
                direction.opposite(),
                pos,
                new_state,
            );
        if neighbor_state != new_neighbor_state {
            let _ = region.set_block_state(
                neighbor_pos,
                new_neighbor_state,
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn update_leaf_shape_at_edge(
        region: &WorldGenRegion<'_>,
        pos: BlockPos,
        state: BlockStateId,
        neighbor_state: BlockStateId,
    ) {
        if !state.get_block().has_tag(&BlockTag::LEAVES) {
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

        let distance_from_neighbor = Self::tree_leaf_distance_at(neighbor_state) + 1;
        if distance_from_neighbor != 1 || distance != distance_from_neighbor {
            let _ = region.schedule_block_tick_default(pos, state.get_block(), 1);
        }
    }

    fn tree_optional_leaf_distance_at(state: BlockStateId) -> Option<u8> {
        if state
            .get_block()
            .has_tag(&BlockTag::PREVENTS_NEARBY_LEAF_DECAY)
        {
            return Some(0);
        }

        state.try_get_value(&BlockStateProperties::DISTANCE)
    }

    fn tree_leaf_distance_at(state: BlockStateId) -> u8 {
        Self::tree_optional_leaf_distance_at(state).unwrap_or(7)
    }

    const fn tree_can_schedule_tick_at(region: &WorldGenRegion<'_>, pos: BlockPos) -> bool {
        region.can_write_to_chunk(
            SectionPos::block_to_section_coord(pos.x()),
            SectionPos::block_to_section_coord(pos.z()),
        )
    }
}

fn take_frontier_position(frontier: &mut JavaBlockPosSet) -> Option<BlockPos> {
    frontier.pop_java_ordered_position()
}
