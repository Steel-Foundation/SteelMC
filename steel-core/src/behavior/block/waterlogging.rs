use super::{
    Arc, BlockBehavior, BlockPos, BlockStateExt, BlockStateId, BlockStateProperties,
    ConditionalBlockSetResult, FluidState, GameType, ItemStack, LevelAccessor, PickupResult,
    Player, ScheduledTickAccess, UpdateFlags, World, sound_events, vanilla_blocks, vanilla_fluids,
    vanilla_items,
};

#[must_use]
pub(crate) fn drained_waterlogged_state(state: BlockStateId) -> Option<BlockStateId> {
    (state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true))
        .then(|| state.set_value(&BlockStateProperties::WATERLOGGED, false))
}

pub(crate) fn pickup_waterlogged_block(
    behavior: &dyn BlockBehavior,
    world: &Arc<World>,
    pos: BlockPos,
    state: BlockStateId,
    player: Option<&Player>,
) -> Option<PickupResult> {
    let new_state = drained_waterlogged_state(state)?;
    if !can_pick_up_drained_waterlogged_state(state, player) {
        return None;
    }

    if world.set_block_if_unchanged(pos, state, new_state, UpdateFlags::UPDATE_ALL)
        != ConditionalBlockSetResult::Changed
    {
        return None;
    }

    if !behavior.can_survive(new_state, world, pos) {
        world.destroy_block(pos, true);
    }

    Some(PickupResult {
        filled_bucket: ItemStack::new(&vanilla_items::WATER_BUCKET),
        sound: Some(&sound_events::ITEM_BUCKET_FILL),
    })
}

pub(super) fn can_pick_up_drained_waterlogged_state(
    state: BlockStateId,
    player: Option<&Player>,
) -> bool {
    // Vanilla BarrierBlock only delegates to SimpleWaterloggedBlock for creative players;
    // sponge passes no user and therefore must not drain it.
    if state.get_block() != &vanilla_blocks::BARRIER {
        return true;
    }

    player.is_some_and(|player| player.game_mode() == GameType::Creative)
}

pub(crate) fn schedule_placed_liquid_tick(
    level: &dyn LevelAccessor,
    pos: BlockPos,
    fluid_state: FluidState,
) {
    let delay = level.fluid_tick_delay(fluid_state.fluid_id);
    level.schedule_fluid_tick_default(pos, fluid_state.fluid_id, delay);
}

/// Mirrors the water-tick side effect shared by Vanilla waterlogged block
/// `updateShape` implementations.
pub(crate) fn schedule_water_tick_if_waterlogged(
    state: BlockStateId,
    level: &dyn ScheduledTickAccess,
    pos: BlockPos,
) {
    if state.try_get_value(&BlockStateProperties::WATERLOGGED) == Some(true) {
        let delay = level.fluid_tick_delay(&vanilla_fluids::WATER);
        level.schedule_fluid_tick_default(pos, &vanilla_fluids::WATER, delay);
    }
}

pub(crate) fn place_simple_waterlogged_liquid(
    level: &dyn LevelAccessor,
    pos: BlockPos,
    state: BlockStateId,
    fluid_state: FluidState,
) -> bool {
    if state.try_get_value(&BlockStateProperties::WATERLOGGED) != Some(false)
        || fluid_state.fluid_id != &vanilla_fluids::WATER
    {
        return false;
    }

    let new_state = state.set_value(&BlockStateProperties::WATERLOGGED, true);
    level.set_block_state(pos, new_state, UpdateFlags::UPDATE_ALL);
    schedule_placed_liquid_tick(level, pos, fluid_state);
    true
}

pub(crate) fn simple_waterlogged_is_liquid_container(state: BlockStateId) -> bool {
    state
        .try_get_value(&BlockStateProperties::WATERLOGGED)
        .is_some()
}
