//! Farmland block implementation.

use std::ptr;

use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_blocks;
use steel_utils::{BlockPos, BlockStateId, types::UpdateFlags};

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::world::World;

/// Maximum moisture level for farmland.
const MAX_MOISTURE: u8 = 7;

/// Behavior for farmland blocks.
///
/// Farmland has a moisture level (0-7) that affects crop growth speed.
/// - Moisture increases to max (7) when near water
/// - Moisture decreases by 1 each random tick when not near water
/// - Farmland turns back to dirt when moisture reaches 0 and no crop is planted
pub struct FarmlandBlock {
    block: BlockRef,
}

impl FarmlandBlock {
    /// Creates a new farmland block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Checks if there is water within a 9x9x2 area centered on the farmland.
    /// Vanilla checks from (-4, 0, -4) to (4, 1, 4) relative to the farmland.
    ///
    /// This checks both water blocks and waterlogged blocks (matching vanilla's
    /// FluidTags.WATER check on fluid state).
    fn is_near_water(world: &World, pos: BlockPos) -> bool {
        for dy in 0..=1 {
            for dx in -4..=4 {
                for dz in -4..=4 {
                    let check_pos = pos.offset(dx, dy, dz);
                    let state = world.get_block_state(&check_pos);

                    // Check if block is water
                    if ptr::eq(state.get_block(), vanilla_blocks::WATER) {
                        return true;
                    }

                    // Check if block is waterlogged
                    if state
                        .try_get_value(&BlockStateProperties::WATERLOGGED)
                        .unwrap_or(false)
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Checks if the block above is a crop that should maintain the farmland.
    /// This prevents farmland from turning to dirt when crops are planted.
    fn should_maintain_farmland(world: &World, pos: BlockPos) -> bool {
        let above = world.get_block_state(&pos.offset(0, 1, 0));
        let block = above.get_block();

        // Check for crops that maintain farmland
        // In vanilla this uses the MAINTAINS_FARMLAND tag
        ptr::eq(block, vanilla_blocks::WHEAT)
            || ptr::eq(block, vanilla_blocks::CARROTS)
            || ptr::eq(block, vanilla_blocks::POTATOES)
            || ptr::eq(block, vanilla_blocks::BEETROOTS)
            || ptr::eq(block, vanilla_blocks::MELON_STEM)
            || ptr::eq(block, vanilla_blocks::PUMPKIN_STEM)
            || ptr::eq(block, vanilla_blocks::ATTACHED_MELON_STEM)
            || ptr::eq(block, vanilla_blocks::ATTACHED_PUMPKIN_STEM)
            || ptr::eq(block, vanilla_blocks::TORCHFLOWER_CROP)
            || ptr::eq(block, vanilla_blocks::PITCHER_CROP)
    }

    /// Turns the farmland into dirt.
    fn turn_to_dirt(world: &World, pos: BlockPos) {
        let dirt_state = vanilla_blocks::DIRT.default_state();
        world.set_block(pos, dirt_state, UpdateFlags::UPDATE_ALL);
    }
}

impl BlockBehaviour for FarmlandBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Farmland is placed with moisture 0
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::MOISTURE, 0u8),
        )
    }

    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        // Farmland always needs random ticks to manage moisture
        true
    }

    fn random_tick(&self, state: BlockStateId, world: &World, pos: BlockPos) {
        let moisture: u8 = state.get_value(&BlockStateProperties::MOISTURE);

        // TODO: Check for rain when weather is implemented
        let is_near_water = Self::is_near_water(world, pos);

        if !is_near_water {
            // Not near water - decrease moisture or turn to dirt
            if moisture > 0 {
                // Decrease moisture by 1
                let new_state = state.set_value(&BlockStateProperties::MOISTURE, moisture - 1);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            } else if !Self::should_maintain_farmland(world, pos) {
                // No moisture and no crop - turn to dirt
                Self::turn_to_dirt(world, pos);
            }
        } else if moisture < MAX_MOISTURE {
            // Near water - hydrate to max
            let new_state = state.set_value(&BlockStateProperties::MOISTURE, MAX_MOISTURE);
            world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        }
    }
}
