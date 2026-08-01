use super::{
    BlockPos, BlockRef, ChunkPos, ChunkStorage, FluidRef, PersistentTick, REGISTRY, RegistryExt,
    SavedTick, TickPriority,
};

impl ChunkStorage {
    /// Converts block ticks to persistent format for saving.
    pub(super) fn block_ticks_to_persistent(
        ticks: Vec<SavedTick<BlockRef>>,
        chunk_pos: ChunkPos,
    ) -> Vec<PersistentTick> {
        ticks
            .into_iter()
            .map(|t| PersistentTick {
                x: (t.pos.0.x - chunk_pos.0.x * 16) as u8,
                y: t.pos.0.y as i16,
                z: (t.pos.0.z - chunk_pos.0.y * 16) as u8,
                delay: t.delay,
                priority: t.priority as i8,
                tick_type: t.tick_type.key.clone(),
            })
            .collect()
    }

    /// Converts fluid ticks to persistent format for saving.
    pub(super) fn fluid_ticks_to_persistent(
        ticks: Vec<SavedTick<FluidRef>>,
        chunk_pos: ChunkPos,
    ) -> Vec<PersistentTick> {
        ticks
            .into_iter()
            .map(|t| PersistentTick {
                x: (t.pos.0.x - chunk_pos.0.x * 16) as u8,
                y: t.pos.0.y as i16,
                z: (t.pos.0.z - chunk_pos.0.y * 16) as u8,
                delay: t.delay,
                priority: t.priority as i8,
                tick_type: t.tick_type.key.clone(),
            })
            .collect()
    }

    /// Reconstructs saved block ticks from persistent data.
    pub(super) fn persistent_to_block_saved_ticks(
        persistent: &[PersistentTick],
        chunk_pos: ChunkPos,
    ) -> Vec<SavedTick<BlockRef>> {
        persistent
            .iter()
            .filter_map(|pt| {
                let block = REGISTRY.blocks.by_key(&pt.tick_type)?;
                let pos = BlockPos::new(
                    chunk_pos.0.x * 16 + i32::from(pt.x),
                    i32::from(pt.y),
                    chunk_pos.0.y * 16 + i32::from(pt.z),
                );
                let priority = TickPriority::by_value(i32::from(pt.priority));
                Some(SavedTick {
                    tick_type: block,
                    pos,
                    delay: pt.delay,
                    priority,
                })
            })
            .collect()
    }

    /// Reconstructs saved fluid ticks from persistent data.
    pub(super) fn persistent_to_fluid_saved_ticks(
        persistent: &[PersistentTick],
        chunk_pos: ChunkPos,
    ) -> Vec<SavedTick<FluidRef>> {
        persistent
            .iter()
            .filter_map(|pt| {
                let fluid = REGISTRY.fluids.by_key(&pt.tick_type)?;
                let pos = BlockPos::new(
                    chunk_pos.0.x * 16 + i32::from(pt.x),
                    i32::from(pt.y),
                    chunk_pos.0.y * 16 + i32::from(pt.z),
                );
                let priority = TickPriority::by_value(i32::from(pt.priority));
                Some(SavedTick {
                    tick_type: fluid,
                    pos,
                    delay: pt.delay,
                    priority,
                })
            })
            .collect()
    }
}
