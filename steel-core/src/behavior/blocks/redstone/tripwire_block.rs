//! Vanilla tripwire sensing, line updates, and shears disarming.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::{
    BlockRef, block_state_ext::BlockStateExt as _, properties::BlockStateProperties,
    properties::Direction, shapes::VoxelShape,
};
use steel_registry::{vanilla_game_events, vanilla_items};
use steel_utils::{
    BlockPos, BlockStateId,
    axis::Axis,
    types::{InteractionHand, UpdateFlags},
};

use super::TripWireHookBlock;
use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, World, game_event_context::GameEventContext};

const WIRE_DISTANCE_MAX: i32 = 42;
const RECHECK_PERIOD: i32 = 10;

/// Vanilla `TripWireBlock` behavior.
#[block_behavior]
pub struct TripWireBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    hook: BlockRef,
}

impl TripWireBlock {
    /// Creates tripwire behavior.
    #[must_use]
    pub const fn new(block: BlockRef, hook: BlockRef) -> Self {
        Self { block, hook }
    }

    fn should_connect_to(&self, state: BlockStateId, direction: Direction) -> bool {
        if state.get_block() == self.hook {
            state.get_value(&BlockStateProperties::HORIZONTAL_FACING) == direction.opposite()
        } else {
            state.get_block() == self.block
        }
    }

    fn set_connection(state: BlockStateId, direction: Direction, connected: bool) -> BlockStateId {
        match direction {
            Direction::North => state.set_value(&BlockStateProperties::NORTH, connected),
            Direction::East => state.set_value(&BlockStateProperties::EAST, connected),
            Direction::South => state.set_value(&BlockStateProperties::SOUTH, connected),
            Direction::West => state.set_value(&BlockStateProperties::WEST, connected),
            Direction::Up | Direction::Down => state,
        }
    }

    fn update_source(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        for direction in [Direction::South, Direction::West] {
            for distance in 1..WIRE_DISTANCE_MAX {
                let test_pos = pos.relative_n(direction, distance);
                let test_state = world.get_block_state(test_pos);
                if test_state.get_block() == self.hook {
                    if test_state.get_value(&BlockStateProperties::HORIZONTAL_FACING)
                        == direction.opposite()
                    {
                        TripWireHookBlock::calculate_state(
                            world,
                            test_pos,
                            test_state,
                            false,
                            true,
                            distance,
                            Some(state),
                        );
                    }
                    break;
                }
                if test_state.get_block() != self.block {
                    break;
                }
            }
        }
    }

    fn check_pressed_for_entity(&self, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        self.set_pressed(world, pos, !entity.is_ignoring_block_triggers());
    }

    fn check_pressed(&self, world: &Arc<World>, pos: BlockPos) {
        let state = world.get_block_state(pos);
        let Some(local_bounds) = state.get_outline_shape_at(pos).bounds() else {
            self.set_pressed(world, pos, false);
            return;
        };
        let bounds = local_bounds.at_block(pos);
        let should_be_pressed = !world
            .get_entities_in_aabb_matching(&bounds, |entity| !entity.is_ignoring_block_triggers())
            .is_empty();
        self.set_pressed(world, pos, should_be_pressed);
    }

    fn set_pressed(&self, world: &Arc<World>, pos: BlockPos, should_be_pressed: bool) {
        let mut state = world.get_block_state(pos);
        let was_pressed = state.get_value(&BlockStateProperties::POWERED);
        if should_be_pressed != was_pressed {
            state = state.set_value(&BlockStateProperties::POWERED, should_be_pressed);
            world.set_block(pos, state, UpdateFlags::UPDATE_ALL);
            self.update_source(world, pos, state);
        }

        if should_be_pressed {
            world.schedule_block_tick_default(pos, self.block, RECHECK_PERIOD);
        } else if was_pressed {
            world.schedule_block_tick_default(pos, self.block, 0);
        }
    }
}

impl BlockBehavior for TripWireBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = context.place_pos();
        let mut state = self.block.default_state();
        for direction in [
            Direction::North,
            Direction::East,
            Direction::South,
            Direction::West,
        ] {
            state = Self::set_connection(
                state,
                direction,
                self.should_connect_to(
                    context.world.get_block_state(pos.relative(direction)),
                    direction,
                ),
            );
        }
        Some(state)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        _world: &dyn ScheduledTickAccess,
        _pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if direction.axis() == Axis::Y {
            state
        } else {
            Self::set_connection(
                state,
                direction,
                self.should_connect_to(neighbor_state, direction),
            )
        }
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if old_state.get_block() != self.block {
            self.update_source(world, pos, state);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if !moved_by_piston {
            self.update_source(
                world,
                pos,
                state.set_value(&BlockStateProperties::POWERED, true),
            );
        }
    }

    fn player_will_destroy(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
    ) -> BlockStateId {
        let held_shears = {
            let inventory = player.inventory.lock();
            let main_hand = inventory.get_item_in_hand(InteractionHand::MainHand);
            !main_hand.is_empty() && main_hand.is(&vanilla_items::SHEARS)
        };
        if held_shears {
            world.set_block(
                pos,
                state.set_value(&BlockStateProperties::DISARMED, true),
                UpdateFlags::UPDATE_NONE,
            );
            world.game_event(
                &vanilla_game_events::SHEAR,
                pos,
                &GameEventContext::new(Some(player), None),
            );
        }
        state
    }

    fn get_entity_inside_collision_shape(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _entity: &dyn Entity,
    ) -> VoxelShape {
        state.get_static_outline_shape()
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if !state.get_value(&BlockStateProperties::POWERED)
            && !world.has_scheduled_block_tick(pos, self.block)
        {
            self.check_pressed_for_entity(world, pos, entity);
        }
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if world
            .get_block_state(pos)
            .get_value(&BlockStateProperties::POWERED)
        {
            self.check_pressed(world, pos);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use glam::DVec3;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_entities};
    use steel_utils::ChunkPos;

    use super::*;
    use crate::behavior::{BLOCK_BEHAVIORS, init_behaviors};
    use crate::entity::SharedEntity;
    use crate::entity::entities::RawEntity;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn entity_inside_powers_wire_and_attached_hooks() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("tripwire_entity_inside");
        let left = BlockPos::new(5, 64, 8);
        let wire_pos = left.east();
        let right = wire_pos.east();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(left));
        assert!(world.set_block(
            left.west(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        assert!(world.set_block(
            right.east(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let left_state = vanilla_blocks::TRIPWIRE_HOOK
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::East);
        let right_state = vanilla_blocks::TRIPWIRE_HOOK
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::West);
        assert!(world.set_block(left, left_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(right, right_state, UpdateFlags::UPDATE_NONE));
        assert!(world.set_block(
            wire_pos,
            vanilla_blocks::TRIPWIRE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        TripWireHookBlock::calculate_state(&world, left, left_state, false, false, -1, None);

        let entity: SharedEntity = Arc::new(RawEntity::new(
            7_002,
            DVec3::new(6.5, 64.0, 8.5),
            Arc::downgrade(&world),
            &vanilla_entities::PIG,
        ));
        let mut effects = InsideBlockEffectCollector::new();
        BLOCK_BEHAVIORS
            .get_behavior(&vanilla_blocks::TRIPWIRE)
            .entity_inside(
                world.get_block_state(wire_pos),
                &world,
                wire_pos,
                entity.as_ref(),
                &mut effects,
                true,
            );

        assert!(
            world
                .get_block_state(wire_pos)
                .get_value(&BlockStateProperties::POWERED)
        );
        assert!(
            world
                .get_block_state(left)
                .get_value(&BlockStateProperties::POWERED)
        );
        assert!(
            world
                .get_block_state(right)
                .get_value(&BlockStateProperties::POWERED)
        );
        assert!(world.has_scheduled_block_tick(wire_pos, &vanilla_blocks::TRIPWIRE));
    }
}
