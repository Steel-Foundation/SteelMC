//! Liquid block behavior (water, lava).
//!
//! Based on vanilla's LiquidBlock.java.
use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, IntProperty};
use steel_registry::client_content::client_fluid;
use steel_registry::fluid::FluidRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_fluid_tags::FluidTag;
use steel_registry::{REGISTRY, RegistryExt as _};
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::types::UpdateFlags;

use steel_registry::sound_events;
use steel_registry::vanilla_items;

use crate::behavior::FLUID_BEHAVIORS;
use crate::behavior::block::{BlockBehavior, PickupResult};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::ai::path::PathComputationType;
use crate::fluid::is_lava_fluid;
use crate::player::Player;
use crate::world::{ConditionalBlockSetResult, ScheduledTickAccess, World};

use super::BubbleColumnBlock;

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

const LEVEL: &IntProperty = &BlockStateProperties::LEVEL;

impl LiquidBlock {
    /// Creates a new liquid block behavior.
    #[must_use]
    pub const fn new(block: BlockRef, fluid: FluidRef) -> Self {
        Self { block, fluid }
    }

    /// Checks if this liquid should spread, letting its fluid react with neighbors first.
    /// Based on vanilla's `LiquidBlock.shouldSpreadLiquid()`.
    ///
    /// Returns `true` if the liquid should spread (schedule tick), or `false` if the
    /// fluid converted its block. Vanilla hardcodes the lava reactions here; they live
    /// in [`FluidBehavior::react_with_neighbors`] so that registered fluids can react too.
    fn should_spread_liquid(&self, world: &Arc<World>, pos: BlockPos) -> bool {
        FLUID_BEHAVIORS
            .get_behavior(self.fluid)
            .react_with_neighbors(world, pos)
    }

    fn should_bubble_column_occupy(state: BlockStateId) -> bool {
        let fluid_state = state.get_fluid_state();
        fluid_state
            .fluid_id
            .has_tag(&FluidTag::BUBBLE_COLUMN_CAN_OCCUPY)
            && fluid_state.is_source()
            && fluid_state.is_full()
    }

    fn try_schedule_bubble_block_column(
        &self,
        ticks: &dyn ScheduledTickAccess,
        pos: BlockPos,
        state_below: BlockStateId,
    ) {
        let block_below = state_below.get_block();
        if block_below.has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_DRAG_DOWN)
            || block_below.has_tag(&BlockTag::ENABLES_BUBBLE_COLUMN_PUSH_UP)
        {
            let _ = ticks.schedule_block_tick_default(pos, self.block, 20);
        }
    }
}

impl BlockBehavior for LiquidBlock {
    fn is_liquid_block(&self) -> bool {
        true
    }

    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        !is_lava_fluid(self.fluid)
    }

    /// Called when the block is placed.
    fn on_place(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        if self.should_spread_liquid(world, pos) {
            let fluid = state.get_fluid_state().fluid_id;
            let delay = FLUID_BEHAVIORS.get_behavior(fluid).tick_delay(world);
            world.schedule_fluid_tick_default(pos, fluid, delay);
        }

        if Self::should_bubble_column_occupy(state) {
            self.try_schedule_bubble_block_column(world, pos, world.get_block_state(pos.below()));
        }
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if Self::should_bubble_column_occupy(state) {
            BubbleColumnBlock::update_column(
                &vanilla_blocks::BUBBLE_COLUMN,
                world,
                pos,
                world.get_block_state(pos.below()),
            );
        }
    }

    /// Called when a neighboring block changes.
    fn handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        if self.should_spread_liquid(world, pos) {
            let fluid = world.get_block_state(pos).get_fluid_state().fluid_id;
            let delay = FLUID_BEHAVIORS.get_behavior(fluid).tick_delay(world);
            world.schedule_fluid_tick_default(pos, fluid, delay);
        }

        if Self::should_bubble_column_occupy(state) {
            self.try_schedule_bubble_block_column(world, pos, world.get_block_state(pos.below()));
        }
    }

    /// Called when a neighbor's shape changes.
    ///
    /// Vanilla parity: `LiquidBlock.updateShape` schedules a tick only when
    /// either the current block or the neighbor contains a source fluid.
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        let fluid_state = state.get_fluid_state();
        let neighbor_fluid = neighbor_state.get_fluid_state();

        if fluid_state.is_source() || neighbor_fluid.is_source() {
            let delay = world.fluid_tick_delay(fluid_state.fluid_id);
            world.schedule_fluid_tick_default(pos, fluid_state.fluid_id, delay);
        }

        if direction == Direction::Down && Self::should_bubble_column_occupy(state) {
            self.try_schedule_bubble_block_column(world, pos, neighbor_state);
        }

        state
    }

    /// Vanilla parity: `LiquidBlock.randomTick` delegates to the fluid.
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        FLUID_BEHAVIORS
            .get_behavior(state.get_fluid_state().fluid_id)
            .random_tick(world, pos);
    }

    fn pickup_block(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        _player: Option<&Player>,
    ) -> Option<PickupResult> {
        if state.try_get_value(LEVEL) != Some(0) {
            return None;
        }
        // Vanilla `LiquidBlock.pickupBlock` returns `fluid.getBucket()`; a fluid
        // without a bucket item (`minecraft:air`) cannot be picked up.
        let bucket = REGISTRY
            .items
            .by_key(&self.fluid.bucket_item)
            .filter(|item| *item != &*vanilla_items::AIR)?;

        let air = REGISTRY.blocks.get_default_state_id(&vanilla_blocks::AIR);
        if world.set_block_if_unchanged(pos, state, air, UpdateFlags::UPDATE_ALL_IMMEDIATE)
            != ConditionalBlockSetResult::Changed
        {
            return None;
        }

        // The pickup sound follows the fluid clients see.
        let sound = if client_fluid(self.fluid).has_tag(&FluidTag::LAVA) {
            &sound_events::ITEM_BUCKET_FILL_LAVA
        } else {
            &sound_events::ITEM_BUCKET_FILL
        };

        Some(PickupResult {
            filled_bucket: ItemStack::new(bucket),
            sound: Some(sound),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::behavior::init_behaviors;
    use steel_registry::{init_vanilla_registry, vanilla_fluids};

    use crate::test_support::TestLevel;

    use super::*;

    #[test]
    fn update_shape_schedules_actual_flowing_fluid_variant() {
        init_vanilla_registry();
        init_behaviors();

        let block = LiquidBlock::new(&vanilla_blocks::WATER, &vanilla_fluids::WATER);
        let state = vanilla_blocks::WATER.default_state().set_value(LEVEL, 1);
        let neighbor_state = vanilla_blocks::WATER.default_state();
        let level = TestLevel::default();

        let updated = block.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::North,
            Direction::North.relative(BlockPos::ZERO),
            neighbor_state,
        );

        assert_eq!(updated, state);
        assert_eq!(
            level
                .scheduled_fluid_ticks
                .borrow()
                .iter()
                .map(|tick| (tick.fluid, tick.delay))
                .collect::<Vec<_>>(),
            vec![(&vanilla_fluids::FLOWING_WATER, 5)]
        );
    }

    #[test]
    fn source_water_above_soul_sand_schedules_bubble_column_tick() {
        init_vanilla_registry();
        init_behaviors();

        let block = LiquidBlock::new(&vanilla_blocks::WATER, &vanilla_fluids::WATER);
        let state = vanilla_blocks::WATER.default_state();
        let level = TestLevel::default();

        let updated = block.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::SOUL_SAND.default_state(),
        );

        assert_eq!(updated, state);
        assert!(
            level
                .scheduled_block_ticks
                .borrow()
                .iter()
                .any(|tick| tick.block == &vanilla_blocks::WATER && tick.delay == 20)
        );
    }

    #[test]
    fn flowing_water_does_not_schedule_bubble_column_tick() {
        init_vanilla_registry();
        init_behaviors();

        let block = LiquidBlock::new(&vanilla_blocks::WATER, &vanilla_fluids::WATER);
        let state = vanilla_blocks::WATER.default_state().set_value(LEVEL, 1);
        let level = TestLevel::default();

        let updated = block.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::SOUL_SAND.default_state(),
        );

        assert_eq!(updated, state);
        assert!(level.scheduled_block_ticks.borrow().is_empty());
    }

    #[test]
    fn liquid_block_behavior_and_ext_identify_liquid_blocks() {
        use crate::behavior::BlockStateBehaviorExt as _;

        init_vanilla_registry();
        init_behaviors();

        let liquid = LiquidBlock::new(&vanilla_blocks::WATER, &vanilla_fluids::WATER);
        assert!(liquid.is_liquid_block());

        assert!(vanilla_blocks::WATER.default_state().is_liquid_block());
        assert!(vanilla_blocks::LAVA.default_state().is_liquid_block());
        assert!(!vanilla_blocks::STONE.default_state().is_liquid_block());
        assert!(!vanilla_blocks::AIR.default_state().is_liquid_block());
    }
}
