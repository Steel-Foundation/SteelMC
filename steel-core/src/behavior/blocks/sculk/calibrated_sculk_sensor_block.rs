//! Vanilla `CalibratedSculkSensorBlock`.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, EnumProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, Direction};

use super::SculkSensorBlock;
use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext};
use crate::block_entity::entities::CalibratedSculkSensorBlockEntity;
use crate::block_entity::BlockEntityTicker;
use crate::entity::Entity;
use crate::entity::ai::path::PathComputationType;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

const FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

/// Vanilla `CalibratedSculkSensorBlock`.
#[block_behavior]
pub struct CalibratedSculkSensorBlock {
    sensor: SculkSensorBlock,
}

impl CalibratedSculkSensorBlock {
    /// Creates calibrated sculk-sensor behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            sensor: SculkSensorBlock::new(block),
        }
    }
}

impl BlockBehavior for CalibratedSculkSensorBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.sensor
                .get_state_for_placement(context)?
                .set_value(FACING, context.horizontal_direction()),
        )
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
        self.sensor
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.sensor.tick(state, world, pos);
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        self.sensor.step_on(state, world, pos, entity);
    }

    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        old_state: BlockStateId,
        moved_by_piston: bool,
    ) {
        self.sensor
            .on_place(state, world, pos, old_state, moved_by_piston);
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.sensor
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn is_signal_source(&self, state: BlockStateId, context: SignalQueryContext) -> bool {
        self.sensor.is_signal_source(state, context)
    }

    fn get_own_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        context: SignalQueryContext,
    ) -> i32 {
        self.sensor.get_own_signal(state, world, pos, context)
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        if direction == state.get_value(FACING) {
            0
        } else {
            self.sensor
                .get_own_signal(state, world, pos, context)
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
        self.sensor
            .get_direct_signal(state, world, pos, direction, context)
    }

    fn has_analog_output_signal(&self, state: BlockStateId) -> bool {
        self.sensor.has_analog_output_signal(state)
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        self.sensor
            .get_analog_output_signal(state, world, pos, direction)
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.sensor.is_pathfindable(state, computation_type)
    }

    fn spawn_after_break(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        tool: &ItemStack,
        drop_experience: bool,
    ) {
        self.sensor
            .spawn_after_break(state, world, pos, tool, drop_experience);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(CalibratedSculkSensorBlockEntity::new(
            level, pos, state,
        )))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::CALIBRATED_SCULK_SENSOR,
        )
    }
}
