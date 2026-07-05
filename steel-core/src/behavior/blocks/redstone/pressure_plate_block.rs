use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelReader, ScheduledTickAccess, World};

/// Shared vanilla behavior for pressure plate blocks
struct BasePressurePlateBehavior {
    block: BlockRef,
}

impl BasePressurePlateBehavior {
    const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn can_survive(world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        let below_state = world.get_block_state(below_pos);
        below_state.is_face_sturdy_for_at(below_pos, Direction::Up, SupportType::Rigid)
            || below_state.is_face_sturdy_for_at(below_pos, Direction::Up, SupportType::Center)
    }

    fn update_shape(
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
    ) -> BlockStateId {
        if direction == Direction::Down && !Self::can_survive(world, pos) {
            return REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        }
        state
    }

    fn update_neighbors(&self, world: &Arc<World>, pos: BlockPos) {
        world.update_neighbors_at(pos, self.block);
        world.update_neighbors_at(pos.below(), self.block);
    }

    // TODO: Implement vanilla checkPressed once World exposes a compatible entity query
    // with EntitySelector.NO_SPECTATORS and isIgnoringBlockTriggers filtering.
    // This should set the new signal, update neighbors, play pressure plate sounds,
    // emit BLOCK_ACTIVATE/BLOCK_DEACTIVATE game events, and schedule the next tick
    // while pressed.
}

/// Shared behavior for unweighted pressure plate blocks
#[block_behavior]
pub struct PressurePlateBlock {
    base: BasePressurePlateBehavior,
}

impl PressurePlateBlock {
    /// Creates a new pressure plate block behavior
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            base: BasePressurePlateBehavior::new(block),
        }
    }

    fn get_signal_for_state(state: BlockStateId) -> i32 {
        if state.get_value(&BlockStateProperties::POWERED) {
            15
        } else {
            0
        }
    }

    fn set_signal_for_state(state: BlockStateId, signal: i32) -> BlockStateId {
        state.set_value(&BlockStateProperties::POWERED, signal > 0)
    }
}

impl BlockBehavior for PressurePlateBlock {
    fn is_possible_to_respawn_in_this(&self, _state: BlockStateId) -> bool {
        true
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BasePressurePlateBehavior::can_survive(world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        BasePressurePlateBehavior::update_shape(state, world, pos, direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_state = Self::set_signal_for_state(self.base.block.default_state(), 0);
        if !self.can_survive(default_state, context.world, context.place_pos) {
            return None;
        }
        Some(default_state)
    }

    fn tick(&self, state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        if Self::get_signal_for_state(state) != 0 {
            // TODO: Call vanilla-equivalent checkPressed for unweighted plates once
            // entity query/filtering exists.
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if !moved_by_piston && Self::get_signal_for_state(state) > 0 {
            self.base.update_neighbors(world, pos);
        }
    }

    // TODO: Mirror vanilla getSignal, getDirectSignal, and isSignalSource once
    // BlockBehavior has a redstone signal API.
}

/// Shared behavior for weighted pressure plate blocks
#[block_behavior]
pub struct WeightedPressurePlateBlock {
    base: BasePressurePlateBehavior,
    #[json_arg(value, json = "max_weight")]
    max_weight: i32,
}

impl WeightedPressurePlateBlock {
    /// Creates a new weighted pressure plate block behavior
    #[must_use]
    pub const fn new(block: BlockRef, max_weight: i32) -> Self {
        Self {
            base: BasePressurePlateBehavior::new(block),
            max_weight,
        }
    }

    /// Returns the scheduled recheck delay for weighted plates
    #[must_use]
    pub const fn pressed_time() -> i32 {
        10
    }

    /// Converts a filtered entity count to the vanilla weighted redstone signal
    #[must_use]
    pub fn signal_strength_from_entity_count(&self, count: i32) -> i32 {
        let count = count.min(self.max_weight);
        if count <= 0 {
            return 0;
        }
        (count * 15 + self.max_weight - 1) / self.max_weight
    }

    fn get_signal_for_state(state: BlockStateId) -> i32 {
        i32::from(state.get_value(&BlockStateProperties::POWER))
    }

    fn set_signal_for_state(state: BlockStateId, signal: u8) -> BlockStateId {
        state.set_value(&BlockStateProperties::POWER, signal)
    }
}

impl BlockBehavior for WeightedPressurePlateBlock {
    fn is_possible_to_respawn_in_this(&self, _state: BlockStateId) -> bool {
        true
    }

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        BasePressurePlateBehavior::can_survive(world, pos)
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        BasePressurePlateBehavior::update_shape(state, world, pos, direction)
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let default_state = Self::set_signal_for_state(self.base.block.default_state(), 0);
        if !self.can_survive(default_state, context.world, context.place_pos) {
            return None;
        }
        Some(default_state)
    }

    fn tick(&self, state: BlockStateId, _world: &Arc<World>, _pos: BlockPos) {
        if Self::get_signal_for_state(state) != 0 {
            // TODO: Call vanilla-equivalent checkPressed for weighted plates once
            // entity query/filtering exists.
        }
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        if !moved_by_piston && Self::get_signal_for_state(state) > 0 {
            self.base.update_neighbors(world, pos);
        }
    }

    // TODO: Mirror vanilla getSignal, getDirectSignal, and isSignalSource once
    // BlockBehavior has a redstone signal API.
}
