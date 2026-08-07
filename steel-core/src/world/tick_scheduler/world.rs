use super::super::*;

impl World {
    /// Schedules a block tick at the given position.
    ///
    /// The tick will fire after `delay` world game ticks with the given priority.
    /// Its deadline continues to age while the loaded chunk is outside simulation distance.
    /// Only one tick per `(pos, block)` pair can be active at a time — duplicates
    /// are silently ignored.
    pub fn schedule_block_tick(
        &self,
        pos: BlockPos,
        block: BlockRef,
        delay: i32,
        priority: super::TickPriority,
    ) {
        let trigger_tick = self.game_time().wrapping_add(i64::from(delay));
        let order = self.scheduled_ticks.next_sub_tick_order();
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            chunk.schedule_block_tick(pos, block, trigger_tick, priority, order);
        });
    }

    /// Schedules a block tick with `Normal` priority.
    pub fn schedule_block_tick_default(&self, pos: BlockPos, block: BlockRef, delay: i32) {
        self.schedule_block_tick(pos, block, delay, super::TickPriority::Normal);
    }

    /// Schedules a fluid tick at the given position.
    ///
    /// The tick will fire after `delay` world game ticks with the given priority.
    /// Its deadline continues to age while the loaded chunk is outside simulation distance.
    /// Only one tick per `(pos, fluid)` pair can be active at a time.
    pub fn schedule_fluid_tick(
        &self,
        pos: BlockPos,
        fluid: FluidRef,
        delay: i32,
        priority: super::TickPriority,
    ) {
        let trigger_tick = self.game_time().wrapping_add(i64::from(delay));
        let order = self.scheduled_ticks.next_sub_tick_order();
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map.with_full_chunk(chunk_pos, |chunk| {
            chunk.schedule_fluid_tick(pos, fluid, trigger_tick, priority, order);
        });
    }

    /// Schedules a fluid tick with `Normal` priority.
    pub fn schedule_fluid_tick_default(&self, pos: BlockPos, fluid: FluidRef, delay: i32) {
        self.schedule_fluid_tick(pos, fluid, delay, super::TickPriority::Normal);
    }

    /// Returns `true` if a block tick is already scheduled for the given `(pos, block)`.
    ///
    /// # Panics
    ///
    /// Panics if a published Full chunk's scheduled-tick container was finalized,
    /// which violates the chunk publication invariant.
    pub fn has_scheduled_block_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                match chunk.has_scheduled_block_tick(pos, block) {
                    Ok(has_tick) => has_tick,
                    Err(error) => {
                        panic!("Full chunk scheduled-tick ownership invariant failed: {error:?}")
                    }
                }
            })
            .unwrap_or(false)
    }

    /// Returns `true` if a fluid tick is already scheduled for the given `(pos, fluid)`.
    ///
    /// # Panics
    ///
    /// Panics if a published Full chunk's scheduled-tick container was finalized,
    /// which violates the chunk publication invariant.
    pub fn has_scheduled_fluid_tick(&self, pos: BlockPos, fluid: FluidRef) -> bool {
        let chunk_pos = Self::chunk_pos_for_block(pos);
        self.chunk_map
            .with_full_chunk(chunk_pos, |chunk| {
                match chunk.has_scheduled_fluid_tick(pos, fluid) {
                    Ok(has_tick) => has_tick,
                    Err(error) => {
                        panic!("Full chunk scheduled-tick ownership invariant failed: {error:?}")
                    }
                }
            })
            .unwrap_or(false)
    }

    pub(crate) fn register_full_chunk_ticks(
        &self,
        chunk: FullChunkRef<'_>,
    ) -> Result<(), super::TickSchedulerError> {
        self.scheduled_ticks.register_chunk(chunk)
    }

    pub(crate) fn unregister_full_chunk_ticks(&self, pos: ChunkPos) {
        self.scheduled_ticks.unregister_chunk(pos);
    }

    #[cfg(test)]
    pub(crate) fn has_registered_full_chunk_ticks(&self, pos: ChunkPos) -> bool {
        self.scheduled_ticks.has_registered_chunk(pos)
    }

    #[cfg(test)]
    pub(crate) fn has_indexed_scheduled_tick_head(&self, pos: ChunkPos) -> bool {
        self.scheduled_ticks.has_indexed_head(pos)
    }

    pub(crate) fn schedule_block_tick_for_chunk(
        &self,
        chunk: FullChunkRef<'_>,
        pos: BlockPos,
        block: BlockRef,
        trigger_tick: i64,
        priority: super::TickPriority,
        sub_tick_order: i64,
    ) -> Result<bool, super::TickSchedulerError> {
        self.scheduled_ticks.schedule_block(
            chunk,
            block,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        )
    }

    pub(crate) fn schedule_fluid_tick_for_chunk(
        &self,
        chunk: FullChunkRef<'_>,
        pos: BlockPos,
        fluid: FluidRef,
        trigger_tick: i64,
        priority: super::TickPriority,
        sub_tick_order: i64,
    ) -> Result<bool, super::TickSchedulerError> {
        self.scheduled_ticks.schedule_fluid(
            chunk,
            fluid,
            pos,
            trigger_tick,
            priority,
            sub_tick_order,
        )
    }

    pub(crate) fn unpack_scheduled_ticks(
        &self,
        pos: ChunkPos,
    ) -> Result<(), super::TickSchedulerError> {
        self.scheduled_ticks.unpack_chunk(pos, self.game_time())
    }

    pub(crate) fn reconcile_active_scheduled_tick_chunks<I>(
        &self,
        active_chunks: I,
    ) -> Result<(), super::TickSchedulerError>
    where
        I: Iterator<Item = ChunkPos> + Clone,
    {
        self.scheduled_ticks.reconcile_active_chunks(active_chunks)
    }

    pub(crate) fn begin_scheduled_tick_phase(
        &self,
        current_tick: i64,
        max_ticks: usize,
    ) -> super::ScheduledTickBatch<BlockRef> {
        self.scheduled_ticks.begin_tick(current_tick, max_ticks)
    }

    pub(crate) fn collect_scheduled_fluid_tick_batch(
        &self,
        current_tick: i64,
        max_ticks: usize,
    ) -> super::ScheduledTickBatch<FluidRef> {
        self.scheduled_ticks
            .collect_fluid_ticks(current_tick, max_ticks)
    }

    /// Returns whether a selected block tick at `(pos, block)` has not started yet.
    ///
    /// This mirrors `LevelTickAccess.willTickThisTick` and is distinct from
    /// [`Self::has_scheduled_block_tick`], because selected ticks have already
    /// been removed from their owning chunk queue.
    pub fn will_tick_block_this_tick(&self, pos: BlockPos, block: BlockRef) -> bool {
        let batch = self
            .scheduled_block_ticks_this_tick
            .lock()
            .as_ref()
            .map(Arc::clone);
        batch.is_some_and(|batch| batch.contains(pos, block))
    }

    /// Returns whether a selected fluid tick at `(pos, fluid)` has not started yet.
    pub fn will_tick_fluid_this_tick(&self, pos: BlockPos, fluid: FluidRef) -> bool {
        let batch = self
            .scheduled_fluid_ticks_this_tick
            .lock()
            .as_ref()
            .map(Arc::clone);
        batch.is_some_and(|batch| batch.contains(pos, fluid))
    }

    pub(crate) fn begin_scheduled_block_tick_batch(
        &self,
        ticks: Vec<super::BlockTick>,
    ) -> Arc<super::ScheduledTickRunBatch<BlockRef>> {
        let batch = Arc::new(super::ScheduledTickRunBatch::new(ticks));
        let mut current = self.scheduled_block_ticks_this_tick.lock();
        assert!(
            current.is_none(),
            "scheduled block-tick batch was already active"
        );
        *current = Some(Arc::clone(&batch));
        batch
    }

    pub(crate) fn end_scheduled_block_tick_batch(
        &self,
        batch: &Arc<super::ScheduledTickRunBatch<BlockRef>>,
    ) {
        let removed = {
            let mut current = self.scheduled_block_ticks_this_tick.lock();
            assert!(
                current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, batch)),
                "scheduled block-tick batch identity changed during execution"
            );
            current.take()
        };
        drop(removed);
    }

    pub(crate) fn begin_scheduled_fluid_tick_batch(
        &self,
        ticks: Vec<super::FluidTick>,
    ) -> Arc<super::ScheduledTickRunBatch<FluidRef>> {
        let batch = Arc::new(super::ScheduledTickRunBatch::new(ticks));
        let mut current = self.scheduled_fluid_ticks_this_tick.lock();
        assert!(
            current.is_none(),
            "scheduled fluid-tick batch was already active"
        );
        *current = Some(Arc::clone(&batch));
        batch
    }

    pub(crate) fn end_scheduled_fluid_tick_batch(
        &self,
        batch: &Arc<super::ScheduledTickRunBatch<FluidRef>>,
    ) {
        let removed = {
            let mut current = self.scheduled_fluid_ticks_this_tick.lock();
            assert!(
                current
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, batch)),
                "scheduled fluid-tick batch identity changed during execution"
            );
            current.take()
        };
        drop(removed);
    }
}
