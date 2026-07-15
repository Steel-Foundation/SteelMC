//! Leaves block behavior implementation.
//!
use std::sync::Arc;

use crate::{
    behavior::{BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt},
    world::{LevelReader, ScheduledTickAccess, World},
};
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, BoolProperty, Direction, IntProperty},
    },
    fluid::FluidStateExt,
    vanilla_block_tags::BlockTag,
    vanilla_blocks, vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

const DISTANCE: IntProperty = BlockStateProperties::DISTANCE;
const PERSISTENT: BoolProperty = BlockStateProperties::PERSISTENT;
const WATERLOGGED: BoolProperty = BlockStateProperties::WATERLOGGED;

/// Shared behavior for vanilla leaves blocks.
pub struct LeavesBlock {
    block: BlockRef,
}

impl LeavesBlock {
    /// Creates a new leaves block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    fn decaying(state: BlockStateId) -> bool {
        !state.get_value(&PERSISTENT) && state.get_value(&DISTANCE) == 7
    }
    fn update_distance(
        state: BlockStateId,
        level: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockStateId {
        let mut new_distance = 7;
        for direction in Direction::ALL {
            let mut neighbor_pos = pos;
            neighbor_pos = neighbor_pos.relative(direction);
            new_distance =
                new_distance.min(Self::get_distance_at(level.get_block_state(neighbor_pos)) + 1);

            if new_distance == 1 {
                break;
            }
        }
        state.set_value(&DISTANCE, new_distance)
    }
    fn get_distance_at(state: BlockStateId) -> u8 {
        Self::get_optional_distance_at(state).unwrap_or(7)
    }
    fn get_optional_distance_at(state: BlockStateId) -> Option<u8> {
        if state
            .get_block()
            .has_tag(&BlockTag::PREVENTS_NEARBY_LEAF_DECAY)
        {
            return Some(0);
        }
        if state.try_get_value(&DISTANCE).is_some() {
            return Some(state.get_value(&DISTANCE));
        }
        None
    }
}

impl BlockBehavior for LeavesBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if Self::decaying(state) {
            world.drop_resources(state, pos);
            //TODO: we should probably call level.removeBlock instead
            world.set_block(
                pos,
                vanilla_blocks::AIR.default_state(),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        world.set_block(
            pos,
            Self::update_distance(state, world, pos),
            UpdateFlags::UPDATE_ALL,
        );
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        if state.get_value(&WATERLOGGED) {
            let delay = world.fluid_tick_delay(&vanilla_fluids::WATER);
            world.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
        }
        let distance_from_neighbor = Self::get_distance_at(neighbor_state) + 1;
        if distance_from_neighbor != 1 || state.get_value(&DISTANCE) != distance_from_neighbor {
            world.schedule_block_tick_default(pos, self.block, 1);
        }
        state
    }
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let replaced_fluid_state = context
            .world
            .get_block_state(context.place_pos)
            .get_fluid_state();
        let state = self
            .block
            .default_state()
            .set_value(&PERSISTENT, true)
            .set_value(&WATERLOGGED, replaced_fluid_state.is_water());
        Some(Self::update_distance(
            state,
            context.world,
            context.place_pos,
        ))
    }
    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }
}
/// Used for cherry tree leaves.
#[block_behavior]
pub struct UntintedParticleLeavesBlock {
    block: BlockRef,
}

impl UntintedParticleLeavesBlock {
    /// Creates new `UntintedParticleLeavesBlock` behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn leaves(&self) -> LeavesBlock {
        LeavesBlock::new(self.block)
    }
}

impl BlockBehavior for UntintedParticleLeavesBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.leaves().random_tick(state, world, pos);
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.leaves().tick(state, world, pos);
    }
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.leaves().get_state_for_placement(context)
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
        self.leaves()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }
    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }
}
/// Used for oak, spruce, jungle... tree leaves.
#[block_behavior]
pub struct TintedParticleLeavesBlock {
    block: BlockRef,
}

impl TintedParticleLeavesBlock {
    /// Creates new `TintedParticleLeavesBlock` behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn leaves(&self) -> LeavesBlock {
        LeavesBlock::new(self.block)
    }
}

impl BlockBehavior for TintedParticleLeavesBlock {
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.leaves().random_tick(state, world, pos);
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.leaves().tick(state, world, pos);
    }
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.leaves().get_state_for_placement(context)
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
        self.leaves()
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        true
    }
}
