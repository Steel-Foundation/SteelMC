use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction, WorldAabb};

use crate::behavior::{BlockBehavior, BlockPlaceContext, RailBehavior};
use crate::entity::{Entity, InsideBlockEffectCollector};
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

use super::base_rail_block::BaseRailBlock;
use super::rail_state::RailState;

/// Vanilla detector rail digital behavior.
#[block_behavior]
pub struct DetectorRailBlock {
    base: BaseRailBlock,
}

impl DetectorRailBlock {
    const PRESSED_CHECK_PERIOD: i32 = 20;
    const SEARCH_INSET: f64 = 0.2;

    /// Creates detector rail behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BaseRailBlock::new(block, true),
        }
    }

    fn search_bounds(pos: BlockPos) -> WorldAabb {
        WorldAabb::new(
            f64::from(pos.x()) + Self::SEARCH_INSET,
            f64::from(pos.y()),
            f64::from(pos.z()) + Self::SEARCH_INSET,
            f64::from(pos.x() + 1) - Self::SEARCH_INSET,
            f64::from(pos.y() + 1) - Self::SEARCH_INSET,
            f64::from(pos.z() + 1) - Self::SEARCH_INSET,
        )
    }

    fn has_interacting_minecart(world: &World, pos: BlockPos) -> bool {
        world.has_entity_in_aabb_matching(&Self::search_bounds(pos), |entity| {
            entity.entity_type().is_abstract_minecart
        })
    }

    fn update_power_to_connected(world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        let Some(rail) = RailState::new(world, pos, state) else {
            return;
        };
        for connection_pos in rail.connections() {
            let connection_state = world.get_block_state(*connection_pos);
            world.neighbor_changed_with_state(
                connection_state,
                *connection_pos,
                connection_state.get_block(),
                false,
            );
        }
    }

    fn check_pressed(&self, world: &Arc<World>, pos: BlockPos, state: BlockStateId) {
        if !BaseRailBlock::can_survive(world.as_ref(), pos) {
            return;
        }

        let was_pressed = state.get_value(&BlockStateProperties::POWERED);
        let should_be_pressed = Self::has_interacting_minecart(world, pos);
        if should_be_pressed != was_pressed {
            let new_state = state.set_value(&BlockStateProperties::POWERED, should_be_pressed);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
            Self::update_power_to_connected(world, pos, new_state);
            world.update_neighbors_at(pos, self.base.block);
            world.update_neighbors_at(pos.below(), self.base.block);
            // Vanilla's setBlocksDirty is client rendering bookkeeping and has
            // no additional dedicated-server side effect.
        }

        if should_be_pressed {
            world.schedule_block_tick_default(pos, self.base.block, Self::PRESSED_CHECK_PERIOD);
        }
        world.update_neighbor_for_output_signal(pos, self.base.block);
    }

    fn signal(state: BlockStateId) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }
}

impl RailBehavior for DetectorRailBlock {
    fn is_straight(&self) -> bool {
        self.base.is_straight()
    }
}

impl BlockBehavior for DetectorRailBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.base.state_for_placement(context))
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BaseRailBlock::can_survive(world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        BaseRailBlock::update_shape(state, world, pos)
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        if old_state.get_block() == self.base.block {
            return;
        }
        let updated = self
            .base
            .update_state_on_place(state, world, pos, moved_by_piston);
        self.check_pressed(world, pos, updated);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        moved_by_piston: bool,
    ) {
        self.base
            .handle_neighbor_changed(state, world, pos, moved_by_piston);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if state.get_value(&BlockStateProperties::POWERED) {
            self.check_pressed(world, pos, state);
        }
    }

    fn entity_inside(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if !state.get_value(&BlockStateProperties::POWERED) {
            self.check_pressed(world, pos, state);
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.base
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        Self::signal(state)
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if direction == Direction::Up {
            Self::signal(state)
        } else {
            0
        }
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        // Command success counts and container fullness require concrete
        // minecart capabilities. No currently implemented Steel minecart
        // exposes either, for which vanilla's observable result is zero.
        0
    }

    fn as_rail(&self) -> Option<&dyn RailBehavior> {
        Some(self)
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
    use crate::entity::entities::RawEntity;
    use crate::entity::{InsideBlockEffectCollector, RemovalReason, SharedEntity};
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn minecart_powers_detector_and_schedules_relative_tick() {
        init_test_registry();
        init_behaviors();
        let world = fresh_test_world("detector_rail_minecart");
        let pos = BlockPos::new(8, 64, 8);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        world.set_block(
            pos.below(),
            vanilla_blocks::STONE.default_state(),
            UpdateFlags::UPDATE_NONE,
        );
        let state = vanilla_blocks::DETECTOR_RAIL.default_state();
        world.set_block(pos, state, UpdateFlags::UPDATE_NONE);

        let minecart: SharedEntity = Arc::new(RawEntity::new(
            8_001,
            DVec3::new(8.5, 64.0, 8.5),
            Arc::downgrade(&world),
            &vanilla_entities::MINECART,
        ));
        world
            .try_add_entity(Arc::clone(&minecart))
            .expect("test minecart should enter loaded chunk");

        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::DETECTOR_RAIL);
        let mut effects = InsideBlockEffectCollector::new();
        behavior.entity_inside(state, &world, pos, minecart.as_ref(), &mut effects, true);

        let powered = world.get_block_state(pos);
        assert!(powered.get_value(&BlockStateProperties::POWERED));
        assert_eq!(
            behavior.get_own_signal(powered, &world, pos, SignalQueryContext::DEFAULT,),
            15
        );
        assert_eq!(
            behavior.get_direct_signal(
                powered,
                &world,
                pos,
                Direction::Up,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
        assert_eq!(
            behavior.get_direct_signal(
                powered,
                &world,
                pos,
                Direction::North,
                SignalQueryContext::DEFAULT,
            ),
            0
        );
        assert!(world.has_scheduled_block_tick(pos, &vanilla_blocks::DETECTOR_RAIL));

        minecart.set_removed(RemovalReason::Discarded);
        behavior.tick(powered, &world, pos);
        assert!(
            !world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::POWERED)
        );
    }
}
