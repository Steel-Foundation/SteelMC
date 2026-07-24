use std::sync::Arc;

use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, RailShape};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::BLOCK_BEHAVIORS;
use crate::world::{LevelReader as _, World};

use super::base_rail_block::BaseRailBlock;

/// Vanilla's ordered, mutable rail connection resolver.
///
/// `connections` intentionally remains a `Vec`: insertion and traversal order
/// affects curve selection and synchronous neighbor updates.
pub(super) struct RailState<'a> {
    world: &'a Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    is_straight: bool,
    connections: Vec<BlockPos>,
}

impl<'a> RailState<'a> {
    pub(super) fn new(world: &'a Arc<World>, pos: BlockPos, state: BlockStateId) -> Option<Self> {
        if !BaseRailBlock::is_rail_state(state) {
            return None;
        }
        let is_straight = BLOCK_BEHAVIORS
            .get_behavior(state.get_block())
            .as_rail()?
            .is_straight();
        let shape = state.get_value(&BlockStateProperties::RAIL_SHAPE);
        let mut rail = Self {
            world,
            pos,
            state,
            is_straight,
            connections: Vec::with_capacity(2),
        };
        rail.update_connections(shape);
        Some(rail)
    }

    #[must_use]
    pub(super) fn connections(&self) -> &[BlockPos] {
        &self.connections
    }

    fn update_connections(&mut self, shape: RailShape) {
        self.connections.clear();
        match shape {
            RailShape::NorthSouth => {
                self.connections.push(self.pos.north());
                self.connections.push(self.pos.south());
            }
            RailShape::EastWest => {
                self.connections.push(self.pos.west());
                self.connections.push(self.pos.east());
            }
            RailShape::AscendingEast => {
                self.connections.push(self.pos.west());
                self.connections.push(self.pos.east().above());
            }
            RailShape::AscendingWest => {
                self.connections.push(self.pos.west().above());
                self.connections.push(self.pos.east());
            }
            RailShape::AscendingNorth => {
                self.connections.push(self.pos.north().above());
                self.connections.push(self.pos.south());
            }
            RailShape::AscendingSouth => {
                self.connections.push(self.pos.north());
                self.connections.push(self.pos.south().above());
            }
            RailShape::SouthEast => {
                self.connections.push(self.pos.east());
                self.connections.push(self.pos.south());
            }
            RailShape::SouthWest => {
                self.connections.push(self.pos.west());
                self.connections.push(self.pos.south());
            }
            RailShape::NorthWest => {
                self.connections.push(self.pos.west());
                self.connections.push(self.pos.north());
            }
            RailShape::NorthEast => {
                self.connections.push(self.pos.east());
                self.connections.push(self.pos.north());
            }
        }
    }

    fn remove_soft_connections(&mut self) {
        let mut index = 0;
        while index < self.connections.len() {
            let Some(rail) = self.get_rail(self.connections[index]) else {
                self.connections.remove(index);
                continue;
            };
            if rail.connects_to(self) {
                self.connections[index] = rail.pos;
                index += 1;
            } else {
                self.connections.remove(index);
            }
        }
    }

    fn has_rail(&self, pos: BlockPos) -> bool {
        BaseRailBlock::is_rail_state(self.world.get_block_state(pos))
            || BaseRailBlock::is_rail_state(self.world.get_block_state(pos.above()))
            || BaseRailBlock::is_rail_state(self.world.get_block_state(pos.below()))
    }

    fn get_rail(&self, pos: BlockPos) -> Option<Self> {
        for test_pos in [pos, pos.above(), pos.below()] {
            let state = self.world.get_block_state(test_pos);
            if let Some(rail) = Self::new(self.world, test_pos, state) {
                return Some(rail);
            }
        }
        None
    }

    fn connects_to(&self, rail: &Self) -> bool {
        self.has_connection(rail.pos)
    }

    fn has_connection(&self, rail_pos: BlockPos) -> bool {
        self.connections
            .iter()
            .any(|pos| pos.x() == rail_pos.x() && pos.z() == rail_pos.z())
    }

    #[must_use]
    pub(super) fn count_potential_connections(&self) -> usize {
        Direction::HORIZONTAL
            .into_iter()
            .filter(|direction| self.has_rail(self.pos.relative(*direction)))
            .count()
    }

    fn can_connect_to(&self, rail: &Self) -> bool {
        self.connects_to(rail) || self.connections.len() != 2
    }

    fn connect_to(&mut self, rail: &Self) {
        self.connections.push(rail.pos);
        let north = self.pos.north();
        let south = self.pos.south();
        let west = self.pos.west();
        let east = self.pos.east();
        let n = self.has_connection(north);
        let s = self.has_connection(south);
        let w = self.has_connection(west);
        let e = self.has_connection(east);

        let mut shape = None;
        if n || s {
            shape = Some(RailShape::NorthSouth);
        }
        if w || e {
            shape = Some(RailShape::EastWest);
        }
        if !self.is_straight {
            if s && e && !n && !w {
                shape = Some(RailShape::SouthEast);
            }
            if s && w && !n && !e {
                shape = Some(RailShape::SouthWest);
            }
            if n && w && !s && !e {
                shape = Some(RailShape::NorthWest);
            }
            if n && e && !s && !w {
                shape = Some(RailShape::NorthEast);
            }
        }

        if shape == Some(RailShape::NorthSouth) {
            if BaseRailBlock::is_rail_state(self.world.get_block_state(north.above())) {
                shape = Some(RailShape::AscendingNorth);
            }
            if BaseRailBlock::is_rail_state(self.world.get_block_state(south.above())) {
                shape = Some(RailShape::AscendingSouth);
            }
        }
        if shape == Some(RailShape::EastWest) {
            if BaseRailBlock::is_rail_state(self.world.get_block_state(east.above())) {
                shape = Some(RailShape::AscendingEast);
            }
            if BaseRailBlock::is_rail_state(self.world.get_block_state(west.above())) {
                shape = Some(RailShape::AscendingWest);
            }
        }

        let shape = shape.unwrap_or(RailShape::NorthSouth);
        self.state = self
            .state
            .set_value(&BlockStateProperties::RAIL_SHAPE, shape);
        self.world
            .set_block(self.pos, self.state, UpdateFlags::UPDATE_ALL);
    }

    fn has_neighbor_rail(&self, rail_pos: BlockPos) -> bool {
        let Some(mut neighbor) = self.get_rail(rail_pos) else {
            return false;
        };
        neighbor.remove_soft_connections();
        neighbor.can_connect_to(self)
    }

    /// Places this rail and synchronously connects neighbors in vanilla order.
    #[expect(
        clippy::too_many_lines,
        reason = "keeping vanilla's sequential shape overwrites together makes their order auditable"
    )]
    pub(super) fn place(
        &mut self,
        has_signal: bool,
        first: bool,
        default_shape: RailShape,
    ) -> BlockStateId {
        let north = self.pos.north();
        let south = self.pos.south();
        let west = self.pos.west();
        let east = self.pos.east();
        let n = self.has_neighbor_rail(north);
        let s = self.has_neighbor_rail(south);
        let w = self.has_neighbor_rail(west);
        let e = self.has_neighbor_rail(east);

        let north_or_south = n || s;
        let west_or_east = w || e;
        let mut shape = None;
        if north_or_south && !west_or_east {
            shape = Some(RailShape::NorthSouth);
        }
        if west_or_east && !north_or_south {
            shape = Some(RailShape::EastWest);
        }

        let south_and_east = s && e;
        let south_and_west = s && w;
        let north_and_east = n && e;
        let north_and_west = n && w;
        if !self.is_straight {
            if south_and_east && !n && !w {
                shape = Some(RailShape::SouthEast);
            }
            if south_and_west && !n && !e {
                shape = Some(RailShape::SouthWest);
            }
            if north_and_west && !s && !e {
                shape = Some(RailShape::NorthWest);
            }
            if north_and_east && !s && !w {
                shape = Some(RailShape::NorthEast);
            }
        }

        if shape.is_none() {
            if north_or_south && west_or_east {
                shape = Some(default_shape);
            } else if north_or_south {
                shape = Some(RailShape::NorthSouth);
            } else if west_or_east {
                shape = Some(RailShape::EastWest);
            }

            if !self.is_straight {
                if has_signal {
                    if south_and_east {
                        shape = Some(RailShape::SouthEast);
                    }
                    if south_and_west {
                        shape = Some(RailShape::SouthWest);
                    }
                    if north_and_east {
                        shape = Some(RailShape::NorthEast);
                    }
                    if north_and_west {
                        shape = Some(RailShape::NorthWest);
                    }
                } else {
                    if north_and_west {
                        shape = Some(RailShape::NorthWest);
                    }
                    if north_and_east {
                        shape = Some(RailShape::NorthEast);
                    }
                    if south_and_west {
                        shape = Some(RailShape::SouthWest);
                    }
                    if south_and_east {
                        shape = Some(RailShape::SouthEast);
                    }
                }
            }
        }

        if shape == Some(RailShape::NorthSouth) {
            if BaseRailBlock::is_rail_state(self.world.get_block_state(north.above())) {
                shape = Some(RailShape::AscendingNorth);
            }
            if BaseRailBlock::is_rail_state(self.world.get_block_state(south.above())) {
                shape = Some(RailShape::AscendingSouth);
            }
        }
        if shape == Some(RailShape::EastWest) {
            if BaseRailBlock::is_rail_state(self.world.get_block_state(east.above())) {
                shape = Some(RailShape::AscendingEast);
            }
            if BaseRailBlock::is_rail_state(self.world.get_block_state(west.above())) {
                shape = Some(RailShape::AscendingWest);
            }
        }

        let shape = shape.unwrap_or(default_shape);
        self.update_connections(shape);
        self.state = self
            .state
            .set_value(&BlockStateProperties::RAIL_SHAPE, shape);
        if first || self.world.get_block_state(self.pos) != self.state {
            self.world
                .set_block(self.pos, self.state, UpdateFlags::UPDATE_ALL);
            for index in 0..self.connections.len() {
                let connection = self.connections[index];
                let Some(mut neighbor) = self.get_rail(connection) else {
                    continue;
                };
                neighbor.remove_soft_connections();
                if neighbor.can_connect_to(self) {
                    neighbor.connect_to(self);
                }
            }
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;
    use steel_registry::vanilla_blocks;
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    fn raw_flags() -> UpdateFlags {
        UpdateFlags::UPDATE_NONE | UpdateFlags::UPDATE_SKIP_ON_PLACE
    }

    fn topology_world(key: &'static str) -> (Arc<World>, BlockPos) {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world(key);
        let center = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(center));
        for offset in [
            BlockPos::ZERO,
            BlockPos::new(0, 0, -1),
            BlockPos::new(0, 0, 1),
            BlockPos::new(-1, 0, 0),
            BlockPos::new(1, 0, 0),
        ] {
            world.set_block(
                center.offset(offset.x(), -1, offset.z()),
                vanilla_blocks::STONE.default_state(),
                raw_flags(),
            );
        }
        (world, center)
    }

    fn set_raw_rail(world: &Arc<World>, pos: BlockPos, shape: RailShape) -> BlockStateId {
        let state = vanilla_blocks::RAIL
            .default_state()
            .set_value(&BlockStateProperties::RAIL_SHAPE, shape);
        world.set_block(pos, state, raw_flags());
        state
    }

    fn set_four_way_junction(world: &Arc<World>, center: BlockPos) -> BlockStateId {
        set_raw_rail(world, center.north(), RailShape::NorthSouth);
        set_raw_rail(world, center.south(), RailShape::NorthSouth);
        set_raw_rail(world, center.west(), RailShape::EastWest);
        set_raw_rail(world, center.east(), RailShape::EastWest);
        set_raw_rail(world, center, RailShape::NorthSouth)
    }

    #[test]
    fn four_way_curve_tie_uses_vanilla_sequential_overwrite_order() {
        let (unpowered_world, center) = topology_world("rail_unpowered_curve_tie");
        let state = set_four_way_junction(&unpowered_world, center);
        let mut rail = RailState::new(&unpowered_world, center, state)
            .expect("ordinary rail should expose rail capability");
        let unpowered = rail.place(false, true, RailShape::NorthSouth);
        assert_eq!(
            unpowered.get_value(&BlockStateProperties::RAIL_SHAPE),
            RailShape::SouthEast
        );

        let (powered_world, center) = topology_world("rail_powered_curve_tie");
        let state = set_four_way_junction(&powered_world, center);
        let mut rail = RailState::new(&powered_world, center, state)
            .expect("ordinary rail should expose rail capability");
        let powered = rail.place(true, true, RailShape::NorthSouth);
        assert_eq!(
            powered.get_value(&BlockStateProperties::RAIL_SHAPE),
            RailShape::NorthWest
        );
    }

    #[test]
    fn east_upper_neighbor_creates_slope_with_ordered_connections() {
        let (world, center) = topology_world("rail_ascending_east");
        world.set_block(
            center.east(),
            vanilla_blocks::STONE.default_state(),
            raw_flags(),
        );
        set_raw_rail(&world, center.east().above(), RailShape::EastWest);
        let state = set_raw_rail(&world, center, RailShape::EastWest);
        let mut rail = RailState::new(&world, center, state)
            .expect("ordinary rail should expose rail capability");
        let placed = rail.place(false, true, RailShape::EastWest);

        assert_eq!(
            placed.get_value(&BlockStateProperties::RAIL_SHAPE),
            RailShape::AscendingEast
        );
        assert_eq!(rail.connections(), &[center.west(), center.east().above()]);
    }
}
