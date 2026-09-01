use super::{
    Arc, BLOCK_BEHAVIORS, BlockStateExt, BlockTick, BlockTickBatchGuard, ChunkMap, ChunkStatus,
    FLUID_BEHAVIORS, FluidTick, FluidTickBatchGuard, MAX_SCHEDULED_TICKS_PER_TICK,
    TickingChunkSnapshot, World,
};

impl ChunkMap {
    /// Collects this tick's block batch from sparse live-container heads.
    pub(super) fn collect_scheduled_block_ticks(
        world: &World,
        tickable_chunks: &TickingChunkSnapshot,
        current_tick: i64,
    ) -> Vec<BlockTick> {
        let collected =
            world.begin_scheduled_tick_phase(current_tick, MAX_SCHEDULED_TICKS_PER_TICK);
        for index in collected.changed_containers {
            let tickable = &tickable_chunks.block[index];
            let Some(chunk) = tickable.holder.try_chunk(ChunkStatus::Full) else {
                panic!("published ticking snapshot lost a Full chunk");
            };
            chunk.mark_dirty();
        }

        collected.ticks
    }

    /// Collects this tick's fluid batch after block callbacks have run.
    pub(super) fn collect_scheduled_fluid_ticks(
        world: &World,
        tickable_chunks: &TickingChunkSnapshot,
        current_tick: i64,
    ) -> Vec<FluidTick> {
        let collected =
            world.collect_scheduled_fluid_tick_batch(current_tick, MAX_SCHEDULED_TICKS_PER_TICK);
        for index in collected.changed_containers {
            let tickable = &tickable_chunks.block[index];
            let Some(chunk) = tickable.holder.try_chunk(ChunkStatus::Full) else {
                panic!("published ticking snapshot lost a Full chunk");
            };
            chunk.mark_dirty();
        }
        collected.ticks
    }

    /// Executes ready scheduled block ticks in their collected order.
    pub(super) fn execute_scheduled_block_ticks(
        world: &Arc<World>,
        ready_block_ticks: Vec<BlockTick>,
    ) {
        if !ready_block_ticks.is_empty() {
            let batch = BlockTickBatchGuard::new(world, ready_block_ticks);
            let block_behaviors = &*BLOCK_BEHAVIORS;
            for (index, tick) in batch.ticks().iter().enumerate() {
                batch.start(index);
                let state = world.get_block_state(tick.pos);
                if state.get_block() != tick.tick_type {
                    continue;
                }
                block_behaviors
                    .get_behavior(tick.tick_type)
                    .tick(state, world, tick.pos);
            }
        }
    }

    /// Executes ready scheduled fluid ticks in their collected order.
    pub(super) fn execute_scheduled_fluid_ticks(
        world: &Arc<World>,
        ready_fluid_ticks: Vec<FluidTick>,
    ) {
        if !ready_fluid_ticks.is_empty() {
            let batch = FluidTickBatchGuard::new(world, ready_fluid_ticks);
            let fluid_behaviors = &*FLUID_BEHAVIORS;
            for (index, tick) in batch.ticks().iter().enumerate() {
                batch.start(index);
                let state = world.get_block_state(tick.pos);
                let fluid_state = state.get_fluid_state();

                // Only execute if the fluid at this location still matches the scheduled tick
                if fluid_state.fluid_id != tick.tick_type {
                    continue;
                }

                fluid_behaviors.get_behavior(tick.tick_type).tick(
                    world,
                    tick.pos,
                    state,
                    fluid_state,
                );
            }
        }
    }
}
