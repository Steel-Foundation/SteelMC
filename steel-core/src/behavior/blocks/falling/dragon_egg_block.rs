use std::sync::Arc;

use rand::random_range;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::Direction;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockPlaceContext};
use crate::world::{LevelAccessor, ScheduledTickAccess, World};

use log::info;

use super::FallingBlock;

const DRAGON_EGG_FALL_DELAY: i32 = 5;

/// Vanilla `Dragon Egg` behavior.
#[block_behavior]
pub struct DragonEggBlock {
    falling: FallingBlock,
}

impl DragonEggBlock {
    /// Creates the server-side behavior for the dragon egg
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self {
            falling: FallingBlock::new(block),
        }
    }

    fn teleport(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        for _ in 0..1000 {
            let x = random_range(0..15) - random_range(0..15);
            let y = random_range(0..7) - random_range(0..7);
            let z = random_range(0..15) - random_range(0..15);

            let new_pos = BlockPos::new(pos.x() + x, pos.y() + y, pos.z() + z);

            if !world.get_block_state(new_pos).is_air() {
                continue;
            }

            if !world.is_block_within_world_border(new_pos) {
                continue;
            }

            if world.is_outside_build_height(y) {
                info!("Outside height limit");
                continue;
            }

            info!("{} {} {}", x, y, z);

            world.set_block_state(new_pos, state, UpdateFlags::UPDATE_CLIENTS);
            world.remove_block(pos, false);
            break;
        }
    }
}

impl BlockBehavior for DragonEggBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.falling.get_state_for_placement(context)
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        let _ = world.schedule_block_tick_default(pos, self.falling.block(), DRAGON_EGG_FALL_DELAY);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let _ = ticks.schedule_block_tick_default(pos, self.falling.block(), DRAGON_EGG_FALL_DELAY);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        BlockBehavior::tick(&self.falling, state, world, pos);
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &crate::inventory::prelude::Player,
        _hit_result: &steel_registry::items::item::BlockHitResult,
        _inv: &mut crate::behavior::InventoryAccess,
    ) -> crate::behavior::InteractionResult {
        self.teleport(state, world, pos);
        crate::behavior::InteractionResult::Success
    }

    fn attack(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &crate::inventory::prelude::Player,
    ) {
        self.teleport(state, world, pos);
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: crate::entity::ai::path::PathComputationType,
    ) -> bool {
        false
    }
}
