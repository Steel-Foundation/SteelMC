//! World, chunk, explosion and piston events.

use std::sync::Arc;

use glam::DVec3;
use steel_utils::{BlockPos, ChunkPos, Direction, Identifier};

use super::{EntityInfo, Event};
use crate::world::World;

/// An explosion is about to destroy blocks. Removing positions from `blocks` spares them;
/// cancelling spares every block (entities are already damaged, as in Bukkit).
pub struct ExplosionEvent {
    /// Exploding world.
    pub world: Arc<World>,
    /// Explosion center.
    pub center: DVec3,
    /// Explosion radius.
    pub radius: f32,
    /// Entity causing the explosion, if any.
    pub source: Option<EntityInfo>,
    /// Blocks the explosion will destroy.
    pub blocks: Vec<BlockPos>,
    /// Whether no block is destroyed.
    pub cancelled: bool,
}

impl Event for ExplosionEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A piston is about to extend or retract.
pub struct PistonEvent {
    /// Piston world.
    pub world: Arc<World>,
    /// Piston position.
    pub pos: BlockPos,
    /// Facing direction.
    pub direction: Direction,
    /// Whether it extends (otherwise it retracts).
    pub extending: bool,
    /// Blocks it will move.
    pub blocks: Vec<BlockPos>,
    /// Whether the piston does not move.
    pub cancelled: bool,
}

impl Event for PistonEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// A chunk became active in a world.
pub struct ChunkLoadEvent {
    /// World key.
    pub world: Identifier,
    /// Chunk position.
    pub pos: ChunkPos,
}

impl Event for ChunkLoadEvent {}

/// A chunk finished unloading.
pub struct ChunkUnloadEvent {
    /// World key.
    pub world: Identifier,
    /// Chunk position.
    pub pos: ChunkPos,
}

impl Event for ChunkUnloadEvent {}

/// A world became available, at startup or when a temporary world is published.
pub struct WorldLoadEvent {
    /// Loaded world.
    pub world: Arc<World>,
}

impl Event for WorldLoadEvent {}

/// A world was retired.
pub struct WorldUnloadEvent {
    /// World key.
    pub world: Identifier,
}

impl Event for WorldUnloadEvent {}

/// The rain or thunder state of a world changed. Cancelling restores the previous state.
pub struct WeatherChangeEvent {
    /// World key.
    pub world: Identifier,
    /// New rain state.
    pub raining: bool,
    /// New thunder state.
    pub thundering: bool,
    /// Whether the previous state is restored.
    pub cancelled: bool,
}

impl Event for WeatherChangeEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}
