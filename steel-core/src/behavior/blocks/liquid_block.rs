//! Liquid block behavior (water, lava).
//!
//! Based on vanilla's LiquidBlock.java.
//!
// TODO: Add support for cached fluid states when FluidState caching is implemented
// TODO: Fix deadlock when calling should_spread_liquid from on_place
//       set_block holds chunk locks when calling on_place, and should_spread_liquid
//       needs to read neighbor block states which requires the same locks -> DEADLOCK

use std::ptr;

use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use crate::behavior::block::BlockBehaviour;
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::{get_fluid_state, is_water_state};
use crate::world::World;

/// Behavior for liquid blocks (water and lava).
///
/// Liquid blocks have a LEVEL property (0-15) that determines the fluid state:
/// - LEVEL 0 = source block (full fluid)
/// - LEVEL 1-7 = flowing fluid with decreasing height
/// - LEVEL 8-15 = falling fluid
pub struct LiquidBlock {
    block: BlockRef,
    fluid: FluidRef,
}

impl LiquidBlock {
    /// Creates a new liquid block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, fluid: FluidRef) -> Self {
        Self { block, fluid }
    }

    /// Checks if this liquid should spread and handles lava-water interactions.
    /// Based on vanilla's `LiquidBlock.shouldSpreadLiquid()`.
    ///
    /// Returns `true` if the liquid should spread (schedule tick),
    /// Returns `false` if the liquid was converted to a block (obsidian/cobblestone/basalt).
    fn should_spread_liquid(&self, world: &World, pos: BlockPos, _state: BlockStateId) -> bool {
        // Only lava has special interactions with water and blue ice
        if !ptr::eq(self.block, vanilla_blocks::LAVA) {
            return true;
        }
        // Check if there's soul soil below (for basalt generation)
        let below_pos = pos.offset(0, -1, 0);
        let below_state = world.get_block_state(&below_pos);
        let has_soul_soil_below = ptr::eq(below_state.get_block(), vanilla_blocks::SOUL_SOIL);

        // Get fluid state to check if this is a source
        let fluid_state = get_fluid_state(world, &pos);

        // Check adjacent directions (Up, North, South, East, West) for water or blue ice
        for direction in [
            Direction::Up,
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
        ] {
            let neighbor_pos = direction.relative(&pos);
            let neighbor_fluid = get_fluid_state(world, &neighbor_pos);

            // Check for water (including flowing_water and waterlogged blocks)
            // Using fluid tag check to support modded fluids registered in the water tag
            if is_water_state(neighbor_fluid) {
                // Lava + Water = Obsidian (if source) or Cobblestone (if flowing)
                let new_block = if fluid_state.is_source() {
                    vanilla_blocks::OBSIDIAN
                } else {
                    vanilla_blocks::COBBLESTONE
                };

                let new_state = REGISTRY.blocks.get_default_state_id(new_block);
                // UPDATE_ALL_IMMEDIATE so neighbor blocks (e.g. the water that triggered
                // this) also receive shape and neighbor-changed updates.
                world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL_IMMEDIATE);
                return false; // Don't schedule fluid tick - block was converted
            }

            // Check for basalt generation: soul soil below + blue ice adjacent
            if has_soul_soil_below {
                let neighbor_state = world.get_block_state(&neighbor_pos);
                if ptr::eq(neighbor_state.get_block(), vanilla_blocks::BLUE_ICE) {
                    let new_state = REGISTRY.blocks.get_default_state_id(vanilla_blocks::BASALT);
                    world.set_block(pos, new_state, UpdateFlags::UPDATE_IMMEDIATE);
                    return false; // Don't schedule fluid tick - block was converted
                }
            }
        }

        true // No interaction occurred, proceed with normal fluid tick
    }
}

impl BlockBehaviour for LiquidBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn get_fluid_state(&self, state: BlockStateId) -> FluidState {
        let level = state.get_value(&BlockStateProperties::LEVEL);
        FluidState::from_block_level(self.fluid, level)
    }

    /// Called when the block is placed.
    fn on_place(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        // NOTE: The old deadlock warning here is no longer valid.
        //
        // The section write lock in LevelChunk::set_block_state is a temporary guard
        // dropped at the end of `.write().set_block_state(…)`, well before on_place is
        // called (level_chunk.rs line ~542). Reading neighbour block states from on_place
        // only acquires read locks, which is perfectly safe under parking_lot's RwLock.
        //
        // Vanilla parity: LiquidBlock.onPlace calls shouldSpreadLiquid immediately so
        // that lava placed or spread next to water converts to cobblestone/obsidian
        // without any visible intermediate lava state.
        if self.should_spread_liquid(world, pos, state) {
            let delay = crate::behavior::FLUID_BEHAVIORS
                .get_behavior(self.fluid)
                .tick_delay(world);
            world.schedule_fluid_tick_default(pos, self.fluid, delay);
        }
    }

    /// Called when a neighboring block changes.
    ///
    /// This is safe to call `should_spread_liquid` here because `handle_neighbor_changed`
    /// is called outside of `set_block` locks (after the block is already placed).
    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        // This is safe because we're not inside set_block locks
        if self.should_spread_liquid(world, pos, state) {
            let delay = crate::behavior::FLUID_BEHAVIORS
                .get_behavior(self.fluid)
                .tick_delay(world);
            world.schedule_fluid_tick_default(pos, self.fluid, delay);
        }
    }

    /// Called when a neighbor's shape changes.
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let fluid_state = self.get_fluid_state(state);
        let neighbor_fluid = crate::fluid::get_fluid_state_from_block(neighbor_state);

        if fluid_state.is_source() || neighbor_fluid.is_source() {
            let delay = crate::behavior::FLUID_BEHAVIORS
                .get_behavior(self.fluid)
                .tick_delay(world);
            world.schedule_fluid_tick_default(pos.clone(), self.fluid, delay);
        }

        state
    }

    fn pickup_block(
        &self,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        _player: Option<&crate::player::Player>,
    ) -> Option<crate::behavior::block::PickupResult> {
        if state.try_get_value(&steel_registry::blocks::properties::BlockStateProperties::LEVEL)
            == Some(0)
        {
            let air = steel_registry::REGISTRY
                .blocks
                .get_default_state_id(steel_registry::vanilla_blocks::AIR);
            world.set_block(pos, air, UpdateFlags::UPDATE_ALL_IMMEDIATE);

            // Give the right bucket based on the liquid type
            let bucket = if std::ptr::eq(self.block, steel_registry::vanilla_blocks::WATER) {
                &steel_registry::vanilla_items::ITEMS.water_bucket
            } else {
                &steel_registry::vanilla_items::ITEMS.lava_bucket
            };

            let sound = if std::ptr::eq(self.block, steel_registry::vanilla_blocks::WATER) {
                steel_registry::sound_events::ITEM_BUCKET_FILL
            } else {
                steel_registry::sound_events::ITEM_BUCKET_FILL_LAVA
            };

            Some(crate::behavior::block::PickupResult {
                resulting_block_state: air,
                filled_bucket: bucket,
                sound: Some(sound),
            })
        } else {
            None
        }
    }
}
