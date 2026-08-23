//! Trapped chest block behavior implementation.
//!
//! A trapped chest behaves like a chest and additionally emits a redstone
//! signal proportional to the number of viewers.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::sound_event::SoundEventRef;
use steel_registry::{vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use super::chest_block::{ChestBehavior, ChestBlock};
use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::blocks::redstone::{MAX_REDSTONE_SIGNAL, MIN_REDSTONE_SIGNAL};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::ChestBlockEntity;
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess, SignalQueryContext, World};

/// Behavior for trapped chest blocks.
#[block_behavior]
pub struct TrappedChestBlock {
    chest: ChestBlock,
    #[json_arg(sound_events, json = "open_sound")]
    open_sound: SoundEventRef,
    #[json_arg(sound_events, json = "close_sound")]
    close_sound: SoundEventRef,
}

impl TrappedChestBlock {
    /// Creates a new trapped chest block behavior.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        open_sound: SoundEventRef,
        close_sound: SoundEventRef,
    ) -> Self {
        Self {
            chest: ChestBlock::with_open_chest_stat(
                block,
                open_sound,
                close_sound,
                &vanilla_custom_stats::TRIGGER_TRAPPED_CHEST,
            ),
            open_sound,
            close_sound,
        }
    }
}

impl ChestBehavior for TrappedChestBlock {
    fn open_sound(&self) -> SoundEventRef {
        self.open_sound
    }

    fn close_sound(&self) -> SoundEventRef {
        self.close_sound
    }
}

impl BlockBehavior for TrappedChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.chest.get_state_for_placement(context)
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
        self.chest
            .update_shape(state, world, pos, direction, neighbor_pos, neighbor_state)
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        self.chest
            .use_without_item(state, world, pos, player, hit_result, inv)
    }

    fn affect_neighbors_after_removal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        moved_by_piston: bool,
    ) {
        self.chest
            .affect_neighbors_after_removal(state, world, pos, moved_by_piston);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.chest.tick(state, world, pos);
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::TRAPPED_CHEST,
            level,
            pos,
            state,
        ))
    }

    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        self.chest.is_pathfindable(state, computation_type)
    }

    fn has_analog_output_signal(&self, state: BlockStateId) -> bool {
        self.chest.has_analog_output_signal(state)
    }

    fn get_analog_output_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
    ) -> i32 {
        self.chest
            .get_analog_output_signal(state, world, pos, direction)
    }

    fn is_signal_source(&self, _state: BlockStateId, _context: SignalQueryContext) -> bool {
        true
    }

    /// Vanilla `TrappedChestBlock.ownSignal`: one level per viewer, capped at 15.
    fn get_own_signal(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _context: SignalQueryContext,
    ) -> i32 {
        world
            .get_block_entity(pos)
            .and_then(|block_entity| {
                block_entity
                    .downcast_ref::<ChestBlockEntity>()
                    .map(ChestBlockEntity::opener_count)
            })
            .unwrap_or(MIN_REDSTONE_SIGNAL)
            .clamp(MIN_REDSTONE_SIGNAL, MAX_REDSTONE_SIGNAL)
    }

    fn get_direct_signal(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        direction: Direction,
        context: SignalQueryContext,
    ) -> i32 {
        if direction == Direction::Up {
            self.get_own_signal(state, world, pos, context)
        } else {
            MIN_REDSTONE_SIGNAL
        }
    }

    fn as_chest(&self) -> Option<&dyn ChestBehavior> {
        Some(self)
    }
}
