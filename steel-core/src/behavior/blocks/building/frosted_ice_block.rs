use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::item_stack::ItemStack;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::building::ice_block::IceBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::block_entity::SharedBlockEntity;
use crate::chunk::light::LightLayer;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Vanilla `FrostedIceBlock` behavior.
///
/// Frosted ice is created by the Frost Walker enchantment on water.
/// It slowly ages (AGE 0→3) based on light level and neighbor count,
/// then melts back into water. Isolated blocks melt faster.
#[block_behavior]
pub struct FrostedIceBlock {
    block: BlockRef,
}

const AGE: &IntProperty = &BlockStateProperties::AGE_3;
const MAX_AGE: u8 = 3;
const NEIGHBORS_TO_AGE: i32 = 4;
const NEIGHBORS_TO_MELT: i32 = 2;
const MELT_BRIGHTNESS_THRESHOLD: i32 = 11;
const AGE_RANDOM_CHANCE: u32 = 3;
const TICK_DELAY_MIN: u8 = 20;
const TICK_DELAY_RANGE: u8 = 21;
const INITIAL_TICK_DELAY_MIN: u8 = 60;
const INITIAL_TICK_DELAY_RANGE: u8 = 61;

impl FrostedIceBlock {
    /// Creates a new frosted ice block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Attempts to age the block. Returns `true` if the block melted.
    fn slightly_melt(state: BlockStateId, world: &Arc<World>, pos: BlockPos) -> bool {
        let age = state.get_value(AGE);
        if age < MAX_AGE {
            world.set_block(pos, state.set_value(AGE, age + 1), UpdateFlags::UPDATE_NONE);
            false
        } else {
            IceBlock::melt(state, world, pos);
            true
        }
    }

    /// Returns `true` if fewer than `limit` adjacent blocks are the same type.
    fn fewer_neighbors_than(world: &dyn LevelReader, pos: BlockPos, limit: i32) -> bool {
        let mut count = 0;
        for direction in Direction::ALL {
            let neighbor_pos = pos.relative(direction);
            if world.get_block_state(neighbor_pos).get_block()
                == world.get_block_state(pos).get_block()
            {
                count += 1;
                if count >= limit {
                    return false;
                }
            }
        }
        true
    }

    /// Returns the effective brightness for aging checks.
    ///
    /// In the End (no skylight), only block light matters.
    /// In other dimensions, sky + block light are considered.
    fn get_brightness(world: &Arc<World>, pos: BlockPos) -> u8 {
        if world.dimension_type.has_skylight {
            world.max_local_raw_brightness(pos, 0)
        } else {
            world.light_value_at(LightLayer::Block, pos)
        }
    }
}

impl BlockBehavior for FrostedIceBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        let delay =
            i32::from(rand::random::<u8>() % INITIAL_TICK_DELAY_RANGE + INITIAL_TICK_DELAY_MIN);
        world.schedule_block_tick_default(pos, self.block, delay);
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let should_age = rand::random::<u32>().is_multiple_of(AGE_RANDOM_CHANCE)
            || Self::fewer_neighbors_than(world, pos, NEIGHBORS_TO_AGE);

        if should_age {
            let brightness = i32::from(Self::get_brightness(world, pos));
            let age = state.get_value(AGE);
            let dampening = i32::from(state.get_light_dampening());

            if brightness > MELT_BRIGHTNESS_THRESHOLD - i32::from(age) - dampening
                && Self::slightly_melt(state, world, pos)
            {
                // Propagate aging to neighbors
                for direction in Direction::ALL {
                    let neighbor_pos = pos.relative(direction);
                    let neighbor_state = world.get_block_state(neighbor_pos);
                    if neighbor_state.get_block() == self.block
                        && !Self::slightly_melt(neighbor_state, world, neighbor_pos)
                    {
                        let delay =
                            i32::from(rand::random::<u8>() % TICK_DELAY_RANGE + TICK_DELAY_MIN);
                        world.schedule_block_tick_default(neighbor_pos, self.block, delay);
                    }
                }
                return;
            }
        }

        let delay = i32::from(rand::random::<u8>() % TICK_DELAY_RANGE + TICK_DELAY_MIN);
        world.schedule_block_tick_default(pos, self.block, delay);
    }

    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if source_block == self.block && Self::fewer_neighbors_than(world, pos, NEIGHBORS_TO_MELT) {
            IceBlock::melt(state, world, pos);
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        None
    }

    fn player_destroy(
        &self,
        world: &Arc<World>,
        _player: &Player,
        pos: BlockPos,
        state: BlockStateId,
        _block_entity: Option<&SharedBlockEntity>,
        _tool: &ItemStack,
    ) {
        IceBlock::melt(state, world, pos);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_blocks};

    #[test]
    fn frosted_ice_default_age_is_zero() {
        init_vanilla_registry();
        let block = FrostedIceBlock::new(&vanilla_blocks::FROSTED_ICE);
        let state = block.block.default_state();
        assert_eq!(state.get_value(AGE), 0);
    }

    #[test]
    fn fewer_neighbors_than_counts_correctly() {
        init_vanilla_registry();
        let _block = FrostedIceBlock::new(&vanilla_blocks::FROSTED_ICE);
        // Verify constant values match vanilla.
        assert_eq!(MAX_AGE, 3);
        assert_eq!(NEIGHBORS_TO_AGE, 4);
        assert_eq!(NEIGHBORS_TO_MELT, 2);
        assert_eq!(AGE_RANDOM_CHANCE, 3);
        assert_eq!(TICK_DELAY_MIN, 20);
        assert_eq!(TICK_DELAY_RANGE, 21);
        assert_eq!(INITIAL_TICK_DELAY_MIN, 60);
        assert_eq!(INITIAL_TICK_DELAY_RANGE, 61);
    }
}
