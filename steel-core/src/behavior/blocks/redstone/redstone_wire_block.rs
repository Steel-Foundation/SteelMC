//! Vanilla non-experimental redstone-wire behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty, RedstoneSide};
use steel_registry::{REGISTRY, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::default_redstone_wire_evaluator::DefaultRedstoneWireEvaluator;
use crate::behavior::{
    BLOCK_BEHAVIORS, BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::player::Player;
use crate::world::{
    LevelReader, ScheduledTickAccess, SignalQueryContext, World, is_redstone_conductor,
};

/// Vanilla `RedStoneWireBlock` using `DefaultRedstoneWireEvaluator`.
///
/// Steel intentionally does not implement the experimental redstone feature.
/// Signal suppression is carried by [`SignalQueryContext`] instead of mutating
/// vanilla's process-global `shouldSignal` field.
#[block_behavior]
pub struct RedStoneWireBlock {
    block: BlockRef,
    cross_state: BlockStateId,
    evaluator: DefaultRedstoneWireEvaluator,
}

impl RedStoneWireBlock {
    /// Creates the ordinary redstone-wire behavior and its persistent evaluator.
    #[must_use]
    pub fn new(block: BlockRef) -> Self {
        let cross_state = block
            .default_state()
            .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
            .set_value(&BlockStateProperties::EAST_REDSTONE, RedstoneSide::Side)
            .set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Side)
            .set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side);
        Self {
            block,
            cross_state,
            evaluator: DefaultRedstoneWireEvaluator::new(block),
        }
    }

    const fn property_for_direction(
        direction: Direction,
    ) -> Option<&'static EnumProperty<RedstoneSide>> {
        match direction {
            Direction::North => Some(&BlockStateProperties::NORTH_REDSTONE),
            Direction::East => Some(&BlockStateProperties::EAST_REDSTONE),
            Direction::South => Some(&BlockStateProperties::SOUTH_REDSTONE),
            Direction::West => Some(&BlockStateProperties::WEST_REDSTONE),
            Direction::Down | Direction::Up => None,
        }
    }

    fn is_connected(side: RedstoneSide) -> bool {
        side != RedstoneSide::None
    }

    fn is_cross(state: BlockStateId) -> bool {
        Direction::HORIZONTAL.into_iter().all(|direction| {
            Self::property_for_direction(direction)
                .is_some_and(|property| Self::is_connected(state.get_value(property)))
        })
    }

    fn is_dot(state: BlockStateId) -> bool {
        Direction::HORIZONTAL.into_iter().all(|direction| {
            Self::property_for_direction(direction)
                .is_some_and(|property| !Self::is_connected(state.get_value(property)))
        })
    }

    fn get_connection_state(
        &self,
        level: &dyn LevelReader,
        state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let was_dot = Self::is_dot(state);
        let mut state = self.get_missing_connections(
            level,
            self.block.default_state().set_value(
                &BlockStateProperties::POWER,
                state.get_value(&BlockStateProperties::POWER),
            ),
            pos,
        );
        if was_dot && Self::is_dot(state) {
            return state;
        }

        let north = Self::is_connected(state.get_value(&BlockStateProperties::NORTH_REDSTONE));
        let south = Self::is_connected(state.get_value(&BlockStateProperties::SOUTH_REDSTONE));
        let east = Self::is_connected(state.get_value(&BlockStateProperties::EAST_REDSTONE));
        let west = Self::is_connected(state.get_value(&BlockStateProperties::WEST_REDSTONE));
        let north_south_empty = !north && !south;
        let east_west_empty = !east && !west;

        if !west && north_south_empty {
            state = state.set_value(&BlockStateProperties::WEST_REDSTONE, RedstoneSide::Side);
        }
        if !east && north_south_empty {
            state = state.set_value(&BlockStateProperties::EAST_REDSTONE, RedstoneSide::Side);
        }
        if !north && east_west_empty {
            state = state.set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side);
        }
        if !south && east_west_empty {
            state = state.set_value(&BlockStateProperties::SOUTH_REDSTONE, RedstoneSide::Side);
        }

        state
    }

    fn get_missing_connections(
        &self,
        level: &dyn LevelReader,
        mut state: BlockStateId,
        pos: BlockPos,
    ) -> BlockStateId {
        let above_state = level.get_block_state(pos.above());
        // Vanilla passes the wire position, rather than `pos.above()`, to this
        // state predicate in `getMissingConnections`.
        let can_connect_up = !is_redstone_conductor(level, above_state, pos);

        for direction in Direction::HORIZONTAL {
            let Some(property) = Self::property_for_direction(direction) else {
                continue;
            };
            if !Self::is_connected(state.get_value(property)) {
                state = state.set_value(
                    property,
                    self.get_connecting_side_with_up(level, pos, direction, can_connect_up),
                );
            }
        }

        state
    }

    fn get_connecting_side(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> RedstoneSide {
        let above_pos = pos.above();
        let can_connect_up = !is_redstone_conductor(level, level.get_block_state(above_pos), pos);
        self.get_connecting_side_with_up(level, pos, direction, can_connect_up)
    }

    fn get_connecting_side_with_up(
        &self,
        level: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        can_connect_up: bool,
    ) -> RedstoneSide {
        let relative_pos = pos.relative(direction);
        let relative_state = level.get_block_state(relative_pos);

        if can_connect_up {
            let behavior = BLOCK_BEHAVIORS.get_behavior(relative_state.get_block());
            let is_placeable_above =
                behavior.is_trapdoor() || Self::can_survive_on(level, relative_pos, relative_state);
            if is_placeable_above
                && self.should_connect_to(level.get_block_state(relative_pos.above()), None)
            {
                if level.is_face_sturdy(relative_state, relative_pos, direction.opposite()) {
                    return RedstoneSide::Up;
                }
                return RedstoneSide::Side;
            }
        }

        if !self.should_connect_to(relative_state, Some(direction))
            && (is_redstone_conductor(level, relative_state, relative_pos)
                || !self.should_connect_to(level.get_block_state(relative_pos.below()), None))
        {
            RedstoneSide::None
        } else {
            RedstoneSide::Side
        }
    }

    fn should_connect_to(&self, state: BlockStateId, direction: Option<Direction>) -> bool {
        if state.get_block() == self.block {
            return true;
        }
        if state.get_block() == &vanilla_blocks::REPEATER {
            let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
            return direction == Some(facing) || direction == Some(facing.opposite());
        }
        if state.get_block() == &vanilla_blocks::OBSERVER {
            return direction == Some(state.get_value(&BlockStateProperties::FACING));
        }

        direction.is_some()
            && BLOCK_BEHAVIORS
                .get_behavior(state.get_block())
                .is_signal_source(state, SignalQueryContext::DEFAULT)
    }

    fn can_survive_on(level: &dyn LevelReader, pos: BlockPos, state: BlockStateId) -> bool {
        level.is_face_sturdy(state, pos, Direction::Up)
            || state.get_block() == &vanilla_blocks::HOPPER
    }

    fn check_corner_change_at(&self, world: &Arc<World>, pos: BlockPos) {
        if world.get_block_state(pos).get_block() != self.block {
            return;
        }

        world.update_neighbors_at(pos, self.block);
        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), self.block);
        }
    }

    fn update_neighbors_of_neighboring_wires(&self, world: &Arc<World>, pos: BlockPos) {
        for direction in Direction::HORIZONTAL {
            self.check_corner_change_at(world, pos.relative(direction));
        }

        for direction in Direction::HORIZONTAL {
            let target = pos.relative(direction);
            let target_state = world.get_block_state(target);
            if is_redstone_conductor(world.as_ref(), target_state, target) {
                self.check_corner_change_at(world, target.above());
            } else {
                self.check_corner_change_at(world, target.below());
            }
        }
    }

    fn updates_on_shape_change(
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        new_state: BlockStateId,
    ) {
        for direction in Direction::HORIZONTAL {
            let Some(property) = Self::property_for_direction(direction) else {
                continue;
            };
            if Self::is_connected(old_state.get_value(property))
                == Self::is_connected(new_state.get_value(property))
            {
                continue;
            }

            let relative_pos = pos.relative(direction);
            let relative_state = world.get_block_state(relative_pos);
            if is_redstone_conductor(world.as_ref(), relative_state, relative_pos) {
                world.update_neighbors_at_except_from_facing(
                    relative_pos,
                    new_state.get_block(),
                    direction.opposite(),
                );
            }
        }
    }
}

impl BlockBehavior for RedStoneWireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.get_connection_state(
            context.world.as_ref(),
            self.cross_state,
            context.place_pos(),
        ))
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction == Direction::Down {
            return if Self::can_survive_on(world, neighbor_pos, neighbor_state) {
                state
            } else {
                REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
            };
        }
        if direction == Direction::Up {
            return self.get_connection_state(world, state, pos);
        }

        let Some(property) = Self::property_for_direction(direction) else {
            return state;
        };
        let side_connection = self.get_connecting_side(world, pos, direction);
        if Self::is_connected(side_connection) == Self::is_connected(state.get_value(property))
            && !Self::is_cross(state)
        {
            state.set_value(property, side_connection)
        } else {
            self.get_connection_state(
                world,
                self.cross_state
                    .set_value(
                        &BlockStateProperties::POWER,
                        state.get_value(&BlockStateProperties::POWER),
                    )
                    .set_value(property, side_connection),
                pos,
            )
        }
    }

    fn update_indirect_neighbour_shapes(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        flags: UpdateFlags,
        update_limit: i32,
    ) {
        for direction in Direction::HORIZONTAL {
            let Some(property) = Self::property_for_direction(direction) else {
                continue;
            };
            if !Self::is_connected(state.get_value(property)) {
                continue;
            }

            let adjacent_pos = pos.relative(direction);
            if world.get_block_state(adjacent_pos).get_block() == self.block {
                continue;
            }

            let below_pos = adjacent_pos.below();
            if world.get_block_state(below_pos).get_block() == self.block {
                let neighbor_pos = below_pos.relative(direction.opposite());
                world.neighbor_shape_changed(
                    direction.opposite(),
                    below_pos,
                    neighbor_pos,
                    world.get_block_state(neighbor_pos),
                    flags,
                    update_limit,
                );
            }

            let above_pos = adjacent_pos.above();
            if world.get_block_state(above_pos).get_block() == self.block {
                let neighbor_pos = above_pos.relative(direction.opposite());
                world.neighbor_shape_changed(
                    direction.opposite(),
                    above_pos,
                    neighbor_pos,
                    world.get_block_state(neighbor_pos),
                    flags,
                    update_limit,
                );
            }
        }
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        Self::can_survive_on(world, below_pos, world.get_block_state(below_pos))
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() == self.block {
            return;
        }

        self.evaluator.update_power_strength(world, pos, state);
        for direction in [Direction::Down, Direction::Up] {
            world.update_neighbors_at(pos.relative(direction), self.block);
        }
        self.update_neighbors_of_neighboring_wires(world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if moved_by_piston {
            return;
        }

        for direction in Direction::ALL {
            world.update_neighbors_at(pos.relative(direction), self.block);
        }
        self.evaluator.update_power_strength(world, pos, state);
        self.update_neighbors_of_neighboring_wires(world, pos);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if self.can_survive(state, world.as_ref(), pos) {
            self.evaluator.update_power_strength(world, pos, state);
        } else {
            world.drop_resources(state, pos);
            world.remove_block(pos, false);
        }
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if !player.abilities.lock().may_build {
            return InteractionResult::Pass;
        }
        if !Self::is_cross(state) && !Self::is_dot(state) {
            return InteractionResult::Pass;
        }

        let new_base_state = if Self::is_cross(state) {
            self.block.default_state()
        } else {
            self.cross_state
        };
        let new_state = self.get_connection_state(
            world.as_ref(),
            new_base_state.set_value(
                &BlockStateProperties::POWER,
                state.get_value(&BlockStateProperties::POWER),
            ),
            pos,
        );
        if new_state == state {
            return InteractionResult::Pass;
        }

        world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
        Self::updates_on_shape_change(world, pos, state, new_state);
        InteractionResult::Success
    }

    fn is_signal_source(&self, _state: BlockStateId, context: SignalQueryContext) -> bool {
        context.wire_signals_enabled()
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::POWER))
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        if !context.wire_signals_enabled() || direction == Direction::Down {
            return 0;
        }

        let power = self.get_own_signal(state, world, pos, context);
        if power == 0 {
            return 0;
        }
        if direction == Direction::Up {
            return power;
        }

        let Some(property) = Self::property_for_direction(direction.opposite()) else {
            return 0;
        };
        if Self::is_connected(
            self.get_connection_state(world, state, pos)
                .get_value(property),
        ) {
            power
        } else {
            0
        }
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        if context.wire_signals_enabled() {
            self.get_signal(state, world, pos, direction, context)
        } else {
            0
        }
    }

    // `animateTick` only creates client-local dust particles; the server has no
    // corresponding work to perform.
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    fn wire() -> RedStoneWireBlock {
        init_test_registry();
        init_behaviors();
        RedStoneWireBlock::new(&vanilla_blocks::REDSTONE_WIRE)
    }

    #[test]
    fn isolated_cross_and_dot_preserve_their_vanilla_shapes() {
        let behavior = wire();
        let level = TestLevel::default().with_block(
            BlockPos::new(0, 63, 0),
            vanilla_blocks::STONE.default_state(),
        );
        let pos = BlockPos::new(0, 64, 0);

        let cross = behavior.get_connection_state(&level, behavior.cross_state, pos);
        let dot = behavior.get_connection_state(
            &level,
            vanilla_blocks::REDSTONE_WIRE.default_state(),
            pos,
        );

        assert!(RedStoneWireBlock::is_cross(cross));
        assert!(RedStoneWireBlock::is_dot(dot));
    }

    #[test]
    fn wire_climbs_sturdy_neighbor_only_when_above_is_connectable() {
        let behavior = wire();
        let pos = BlockPos::new(0, 64, 0);
        let level = TestLevel::default()
            .with_block(pos.below(), vanilla_blocks::STONE.default_state())
            .with_block(pos.east(), vanilla_blocks::STONE.default_state())
            .with_block(pos.east().above(), behavior.block.default_state());

        assert_eq!(
            behavior.get_connecting_side(&level, pos, Direction::East),
            RedstoneSide::Up
        );

        level.set_test_block(pos.above(), vanilla_blocks::STONE.default_state());
        assert_eq!(
            behavior.get_connecting_side(&level, pos, Direction::East),
            RedstoneSide::None
        );
    }

    #[test]
    fn powered_wire_signal_follows_recomputed_connections() {
        let behavior = wire();
        let pos = BlockPos::new(0, 64, 0);
        let state = behavior
            .block
            .default_state()
            .set_value(&BlockStateProperties::POWER, 9)
            .set_value(&BlockStateProperties::NORTH_REDSTONE, RedstoneSide::Side)
            .set_value(&BlockStateProperties::EAST_REDSTONE, RedstoneSide::Side);
        let level = TestLevel::default()
            .with_block(pos.below(), vanilla_blocks::STONE.default_state())
            .with_block(pos.north(), behavior.block.default_state())
            .with_block(pos.east(), behavior.block.default_state());

        assert_eq!(
            behavior.get_signal(
                state,
                &level,
                pos,
                Direction::West,
                SignalQueryContext::DEFAULT,
            ),
            9
        );
        assert_eq!(
            behavior.get_signal(
                state,
                &level,
                pos,
                Direction::East,
                SignalQueryContext::DEFAULT,
            ),
            0
        );
        assert_eq!(
            behavior.get_signal(
                state,
                &level,
                pos,
                Direction::Up,
                SignalQueryContext::without_wire_signals(),
            ),
            0
        );
    }
}
