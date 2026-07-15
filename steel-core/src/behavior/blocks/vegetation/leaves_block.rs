//! Leaves block behavior implementation.
//!
use std::sync::Arc;

use rand::Rng;

use crate::{
    behavior::{
        BlockBehavior, BlockPlaceContext, BlockStateBehaviorExt,
        blocks::vegetation::bonemealable::Bonemealable,
    },
    fluid::fluid_state_to_block,
    world::{LevelReader, ScheduledTickAccess, World},
};
use steel_macros::block_behavior;
use steel_registry::{
    blocks::{
        BlockRef,
        block_state_ext::BlockStateExt as _,
        properties::{BlockStateProperties, BoolProperty, Direction, IntProperty},
    },
    vanilla_block_tags::BlockTag,
    vanilla_fluids,
};
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use super::MangrovePropaguleBlock;

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

    fn decayed_replacement(state: BlockStateId) -> BlockStateId {
        fluid_state_to_block(state.get_fluid_state())
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
            world.set_block(
                pos,
                Self::decayed_replacement(state),
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
        let state = self
            .block
            .default_state()
            .set_value(&PERSISTENT, true)
            .set_value(&WATERLOGGED, context.is_water_source());
        Some(Self::update_distance(
            state,
            context.world,
            context.place_pos(),
        ))
    }
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        Self::decaying(state)
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
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        self.leaves().is_randomly_ticking(state)
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

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        self.leaves().is_randomly_ticking(state)
    }
}

/// Mangrove leaves behavior, including hanging propagule growth.
#[block_behavior]
pub struct MangroveLeavesBlock {
    block: BlockRef,
}

impl MangroveLeavesBlock {
    /// Creates new `MangroveLeavesBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn leaves(&self) -> LeavesBlock {
        LeavesBlock::new(self.block)
    }
}

impl BlockBehavior for MangroveLeavesBlock {
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

    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        self.leaves().is_randomly_ticking(state)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}

impl Bonemealable for MangroveLeavesBlock {
    fn is_valid_bonemeal_target(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> bool {
        world.get_block_state(pos.below()).is_air()
    }

    fn perform_bonemeal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos.below(),
            MangrovePropaguleBlock::create_new_hanging_propagule(),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_blocks};

    use crate::{
        behavior::{BLOCK_BEHAVIORS, init_behaviors},
        test_support::TestLevel,
    };

    use super::*;

    #[test]
    fn leaves_only_randomly_tick_while_decaying() {
        init_test_registry();
        let behavior = LeavesBlock::new(&vanilla_blocks::OAK_LEAVES);
        let decaying = vanilla_blocks::OAK_LEAVES.default_state();

        assert!(behavior.is_randomly_ticking(decaying));
        assert!(!behavior.is_randomly_ticking(decaying.set_value(&DISTANCE, 6)));
        assert!(!behavior.is_randomly_ticking(decaying.set_value(&PERSISTENT, true)));
    }

    #[test]
    fn waterlogged_leaves_decay_into_water() {
        init_test_registry();
        init_behaviors();
        let state = vanilla_blocks::OAK_LEAVES
            .default_state()
            .set_value(&WATERLOGGED, true);

        let replacement = LeavesBlock::decayed_replacement(state);

        assert_eq!(replacement.get_block(), &vanilla_blocks::WATER);
    }

    #[test]
    fn distance_updates_from_decay_preventing_blocks() {
        init_test_registry();
        let level = TestLevel::default().with_block(
            BlockPos::ZERO.relative(Direction::East),
            vanilla_blocks::OAK_LOG.default_state(),
        );

        let updated = LeavesBlock::update_distance(
            vanilla_blocks::OAK_LEAVES.default_state(),
            &level,
            BlockPos::ZERO,
        );

        assert_eq!(updated.get_value(&DISTANCE), 1);
    }

    #[test]
    fn mangrove_leaves_register_bonemeal_behavior() {
        init_test_registry();
        init_behaviors();
        let behavior = BLOCK_BEHAVIORS.get_behavior(&vanilla_blocks::MANGROVE_LEAVES);

        assert!(behavior.as_bonemealable().is_some());
    }

    #[test]
    fn mangrove_leaves_require_air_below_for_bonemeal() {
        init_test_registry();
        let behavior = MangroveLeavesBlock::new(&vanilla_blocks::MANGROVE_LEAVES);
        let state = vanilla_blocks::MANGROVE_LEAVES.default_state();
        let empty_level = TestLevel::default();
        assert!(behavior.is_valid_bonemeal_target(state, &empty_level, BlockPos::ZERO));

        let blocked_level = TestLevel::default().with_block(
            BlockPos::ZERO.below(),
            vanilla_blocks::STONE.default_state(),
        );
        assert!(!behavior.is_valid_bonemeal_target(state, &blocked_level, BlockPos::ZERO));
    }
}
