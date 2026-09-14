//! What a behavior or sensor is handed when the brain runs it.

use std::sync::Arc;

use super::memory::Memories;
use crate::entity::PathfinderMob;
use crate::world::World;

/// The mob a brain is running on, its memories, and the current tick.
pub struct BrainContext<'brain> {
    /// The world the mob is in.
    pub level: &'brain Arc<World>,
    /// The mob whose brain is running.
    pub mob: &'brain dyn PathfinderMob,
    /// The mob's memories.
    pub memories: &'brain mut Memories,
    /// Vanilla `level.getGameTime()` for this tick.
    pub time: i64,
}
