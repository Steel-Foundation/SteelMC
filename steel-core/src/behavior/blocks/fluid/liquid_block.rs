//! Liquid block behavior (water, lava).
//!
//! Based on vanilla's LiquidBlock.java.
//!
// TODO: Add support for cached fluid states when FluidState caching is implemented
use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::fluid::{FluidRef, FluidState};
use steel_registry::vanilla_blocks;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use steel_registry::level_events;
use steel_registry::sound_events;
use steel_registry::vanilla_items;

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::FLUID_BEHAVIORS;
use crate::behavior::block::{BlockBehaviour, PickupResult};
use crate::behavior::context::BlockPlaceContext;
use crate::fluid::{FluidStateExt, is_lava_fluid, is_water_fluid};
use crate::player::Player;
use crate::world::World;

/// Behavior for liquid blocks (water and lava).
///
/// Liquid blocks have a LEVEL property (0-15) that determines the fluid state:
/// - LEVEL 0 = source block (full fluid)
/// - LEVEL 1-7 = flowing fluid with decreasing height
/// - LEVEL 8-15 = falling fluid
#[block_behavior]
pub struct LiquidBlock {
    block: BlockRef,
    #[json_arg(vanilla_fluids, ref)]
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
    fn should_spread_liquid(&self, world: &World, pos: BlockPos) -> bool {
        // Only lava has special interactions with water and blue ice
        if !is_lava_fluid(self.fluid) {
            return true;
        }
        // Check if there's soul soil below (for basalt generation)
        let below_pos = pos.offset(0, -1, 0);
        let below_state = world.get_block_state(&below_pos);
        let has_soul_soil_below = below_state.get_block() == vanilla_blocks::SOUL_SOIL;

        // Get fluid state to check if this is a source
        let fluid_state = world.get_block_state(&pos).get_fluid_state();

        for direction in Direction::FLOW_NEIGHBOR_CHECK {
            let neighbor_pos = direction.relative(&pos);
            let neighbor_fluid = world.get_block_state(&neighbor_pos).get_fluid_state();

            // Check for water (including flowing_water and waterlogged blocks)
            // Using fluid tag check to support modded fluids registered in the water tag
            if neighbor_fluid.is_water() {
                // Lava + Water = Obsidian (if source) or Cobblestone (if flowing)
                let new_block = if fluid_state.is_source() {
                    vanilla_blocks::OBSIDIAN
                } else {
                    vanilla_blocks::COBBLESTONE
                };

                let new_state = REGISTRY.blocks.get_default_state_id(new_block);
                world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
                world.level_event(level_events::LAVA_FIZZ, pos, 0, None);
                return false; // Don't schedule fluid tick - block was converted
            }

            // Check for basalt generation: soul soil below + blue ice adjacent
            if has_soul_soil_below {
                let neighbor_state = world.get_block_state(&neighbor_pos);
                if neighbor_state.get_block() == vanilla_blocks::BLUE_ICE {
                    let new_state = REGISTRY.blocks.get_default_state_id(vanilla_blocks::BASALT);
                    world.set_block(pos, new_state, UpdateFlags::UPDATE_ALL);
                    world.level_event(level_events::LAVA_FIZZ, pos, 0, None);
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
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if self.should_spread_liquid(world, pos) {
            let delay = FLUID_BEHAVIORS.get_behavior(self.fluid).tick_delay(world);
            world.schedule_fluid_tick_default(pos, self.fluid, delay);
        }
    }

    /// Called when a neighboring block changes.
    fn handle_neighbor_changed(
        &self,
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if self.should_spread_liquid(world, pos) {
            let delay = FLUID_BEHAVIORS.get_behavior(self.fluid).tick_delay(world);
            world.schedule_fluid_tick_default(pos, self.fluid, delay);
        }
    }

    /// Called when a neighbor's shape changes.
    ///
    /// Vanilla parity: `LiquidBlock.updateShape` schedules a tick only when
    /// either the current block or the neighbor contains a source fluid.
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let fluid_state =
            FluidState::from_block_level(self.fluid, state.get_value(&BlockStateProperties::LEVEL));
        let neighbor_fluid = neighbor_state.get_fluid_state();

        if fluid_state.is_source() || neighbor_fluid.is_source() {
            let delay = FLUID_BEHAVIORS.get_behavior(self.fluid).tick_delay(world);
            world.schedule_fluid_tick_default(pos, self.fluid, delay);
        }

        state
    }

    /// Vanilla parity: `LiquidBlock.isRandomlyTicking` delegates to the fluid.
    fn is_randomly_ticking(&self, _state: BlockStateId) -> bool {
        FLUID_BEHAVIORS
            .get_behavior(self.fluid)
            .is_randomly_ticking()
    }

    /// Vanilla parity: `LiquidBlock.randomTick` delegates to the fluid.
    fn random_tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        FLUID_BEHAVIORS
            .get_behavior(self.fluid)
            .random_tick(world, pos);
    }

    fn pickup_block(
        &self,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        _player: Option<&Player>,
    ) -> Option<PickupResult> {
        if state.try_get_value(&BlockStateProperties::LEVEL) != Some(0) {
            return None;
        }

        let air = REGISTRY.blocks.get_default_state_id(vanilla_blocks::AIR);
        world.set_block(pos, air, UpdateFlags::UPDATE_ALL_IMMEDIATE);

        let bucket = if is_water_fluid(self.fluid) {
            &vanilla_items::ITEMS.water_bucket
        } else {
            &vanilla_items::ITEMS.lava_bucket
        };

        let sound = if is_water_fluid(self.fluid) {
            sound_events::ITEM_BUCKET_FILL
        } else {
            sound_events::ITEM_BUCKET_FILL_LAVA
        };

        Some(PickupResult {
            filled_bucket: bucket,
            sound: Some(sound),
        })
    }
}
