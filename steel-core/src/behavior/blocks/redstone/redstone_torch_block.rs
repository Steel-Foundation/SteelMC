//! Standing and wall redstone torches, including vanilla burnout behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::blocks::shapes::SupportType;
use steel_registry::{REGISTRY, level_events, vanilla_blocks};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{
    LevelReader, ScheduledTickAccess, SignalQueryContext, World, get_signal as get_redstone_signal,
};

const TOGGLE_DELAY: i32 = 2;
const RESTART_DELAY: i32 = 160;

fn notify_neighbors(block: BlockRef, world: &Arc<World>, pos: BlockPos) {
    for direction in Direction::ALL {
        world.update_neighbors_at(pos.relative(direction), block);
    }
}

fn on_place(block: BlockRef, world: &Arc<World>, pos: BlockPos) {
    notify_neighbors(block, world, pos);
}

fn affect_neighbors_after_removal(
    block: BlockRef,
    world: &Arc<World>,
    pos: BlockPos,
    moved_by_piston: bool,
) {
    if !moved_by_piston {
        notify_neighbors(block, world, pos);
    }
}

fn handle_neighbor_changed(
    block: BlockRef,
    state: BlockStateId,
    world: &Arc<World>,
    pos: BlockPos,
    has_neighbor_signal: bool,
) {
    if state.get_value(&BlockStateProperties::LIT) == has_neighbor_signal
        && !world.will_tick_block_this_tick(pos, block)
    {
        world.schedule_block_tick_default(pos, block, TOGGLE_DELAY);
    }
}

fn tick_torch(state: BlockStateId, world: &Arc<World>, pos: BlockPos, has_neighbor_signal: bool) {
    world.prune_recent_redstone_torch_toggles();

    if state.get_value(&BlockStateProperties::LIT) {
        if !has_neighbor_signal {
            return;
        }

        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::LIT, false),
            UpdateFlags::UPDATE_ALL,
        );
        if world.redstone_torch_toggled_too_frequently(pos, true) {
            world.level_event(level_events::REDSTONE_TORCH_BURNOUT, pos, 0, None);
            let current_block = world.get_block_state(pos).get_block();
            world.schedule_block_tick_default(pos, current_block, RESTART_DELAY);
        }
        return;
    }

    if !has_neighbor_signal && !world.redstone_torch_toggled_too_frequently(pos, false) {
        world.set_block(
            pos,
            state.set_value(&BlockStateProperties::LIT, true),
            UpdateFlags::UPDATE_ALL,
        );
    }
}

fn own_signal(state: BlockStateId) -> i32 {
    if state.get_value(&BlockStateProperties::LIT) {
        15
    } else {
        0
    }
}

/// Standing redstone torch (`redstone_torch`).
#[block_behavior]
pub struct RedstoneTorchBlock {
    block: BlockRef,
}

impl RedstoneTorchBlock {
    /// Creates a standing redstone-torch behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn has_neighbor_signal(world: &dyn LevelReader, pos: BlockPos) -> bool {
        get_redstone_signal(
            world,
            pos.below(),
            Direction::Down,
            SignalQueryContext::DEFAULT,
        ) > 0
    }
}

impl BlockBehavior for RedstoneTorchBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below_pos = pos.below();
        world.is_face_sturdy_for(
            world.get_block_state(below_pos),
            below_pos,
            Direction::Up,
            SupportType::Center,
        )
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
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        } else {
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let state = self.block.default_state();
        self.can_survive(state, context.world.as_ref(), context.place_pos())
            .then_some(state)
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        on_place(self.block, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        affect_neighbors_after_removal(self.block, world, pos, moved_by_piston);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        handle_neighbor_changed(
            self.block,
            state,
            world,
            pos,
            Self::has_neighbor_signal(world.as_ref(), pos),
        );
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        tick_torch(
            state,
            world,
            pos,
            Self::has_neighbor_signal(world.as_ref(), pos),
        );
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
        own_signal(state)
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if direction == Direction::Up {
            0
        } else {
            own_signal(state)
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
        if direction == Direction::Down {
            self.get_signal(state, world, pos, direction, context)
        } else {
            0
        }
    }

    // `animateTick` emits client-local dust particles only.
}

/// Wall redstone torch (`redstone_wall_torch`).
#[block_behavior]
pub struct RedstoneWallTorchBlock {
    block: BlockRef,
}

impl RedstoneWallTorchBlock {
    /// Creates a wall redstone-torch behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    fn has_neighbor_signal(state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let opposite = state
            .get_value(&BlockStateProperties::HORIZONTAL_FACING)
            .opposite();
        get_redstone_signal(
            world,
            pos.relative(opposite),
            opposite,
            SignalQueryContext::DEFAULT,
        ) > 0
    }
}

impl BlockBehavior for RedstoneWallTorchBlock {
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let support_pos = pos.relative(facing.opposite());
        world.is_face_sturdy(world.get_block_state(support_pos), support_pos, facing)
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
        let facing = state.get_value(&BlockStateProperties::HORIZONTAL_FACING);
        if direction.opposite() == facing && !self.can_survive(state, world, pos) {
            REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR)
        } else {
            state
        }
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        for direction in context.get_nearest_looking_directions() {
            if !direction.is_horizontal() {
                continue;
            }
            let state = self.block.default_state().set_value(
                &BlockStateProperties::HORIZONTAL_FACING,
                direction.opposite(),
            );
            if self.can_survive(state, context.world.as_ref(), context.place_pos()) {
                return Some(state);
            }
        }
        None
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        on_place(self.block, world, pos);
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        affect_neighbors_after_removal(self.block, world, pos, moved_by_piston);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        handle_neighbor_changed(
            self.block,
            state,
            world,
            pos,
            Self::has_neighbor_signal(state, world.as_ref(), pos),
        );
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        tick_torch(
            state,
            world,
            pos,
            Self::has_neighbor_signal(state, world.as_ref(), pos),
        );
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
        own_signal(state)
    }

    fn get_signal(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
        direction: Direction,
        _context: SignalQueryContext,
    ) -> i32 {
        if state.get_value(&BlockStateProperties::HORIZONTAL_FACING) == direction {
            0
        } else {
            own_signal(state)
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
        if direction == Direction::Down {
            self.get_signal(state, world, pos, direction, context)
        } else {
            0
        }
    }

    // `animateTick` emits client-local dust particles only.
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::test_support::TestLevel;

    #[test]
    fn standing_torch_reads_power_from_its_support() {
        init_test_registry();
        init_behaviors();
        let pos = BlockPos::new(0, 64, 0);
        let powered = TestLevel::default()
            .with_block(pos.below(), vanilla_blocks::REDSTONE_BLOCK.default_state());
        let unpowered =
            TestLevel::default().with_block(pos.below(), vanilla_blocks::STONE.default_state());

        assert!(RedstoneTorchBlock::has_neighbor_signal(&powered, pos));
        assert!(!RedstoneTorchBlock::has_neighbor_signal(&unpowered, pos));
    }

    #[test]
    fn wall_torch_omits_weak_signal_toward_its_support() {
        init_test_registry();
        init_behaviors();
        let behavior = RedstoneWallTorchBlock::new(&vanilla_blocks::REDSTONE_WALL_TORCH);
        let state = vanilla_blocks::REDSTONE_WALL_TORCH
            .default_state()
            .set_value(&BlockStateProperties::HORIZONTAL_FACING, Direction::East)
            .set_value(&BlockStateProperties::LIT, true);
        let level = TestLevel::default();
        let pos = BlockPos::new(0, 64, 0);

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
                Direction::West,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
        assert_eq!(
            behavior.get_direct_signal(
                state,
                &level,
                pos,
                Direction::Down,
                SignalQueryContext::DEFAULT,
            ),
            15
        );
    }
}
