use std::{io, sync::Weak};

use rustc_hash::FxHashMap;
use steel_utils::{ChunkPos, locks::AsyncRwLock};

use crate::chunk::chunk_access::{ChunkAccess, ChunkStatus};
use crate::world::World;

use super::{ChunkStorage, PreparedChunkSave};

/// In-memory chunk storage.
///
/// This storage implementation doesn't
/// persist any data to disk. It's designed for test worlds and minigame worlds:
/// - It has chunk generation which can be disabled with `EmptyChunkGen`
/// - It will save all the data so perfectly for minigames
///
/// TODO:
/// Will later have the option to load a world from storage and clone it for easy world handling
pub struct RamOnlyStorage {
    /// This saves every chunk, and it saves the changes in the world to make it possible to run the server fully in memory
    saved_chunks: AsyncRwLock<FxHashMap<ChunkPos, SimpleRAMChunk>>,
}

/// Represents a simple in-memory chunk containing a prepared chunk save and its status.
///
/// This structure is used to manage the in-memory representation of a chunk in a system,
/// including its saved data and current processing or usage status.
pub struct SimpleRAMChunk {
    /// A `PreparedChunkSave` instance that holds the saved state of the chunk.
    pub prepared: PreparedChunkSave,
    /// A `ChunkStatus` value representing the current status of the chunk.
    pub chunk_status: ChunkStatus,
}

impl RamOnlyStorage {
    /// Creates a new RAM-only storage which can be used for minigames, etc.
    ///
    /// This should be used for a RAM storage solution of a map and every world generation should be supported
    #[must_use]
    pub fn empty_world() -> Self {
        Self {
            saved_chunks: AsyncRwLock::new(FxHashMap::default()),
        }
    }

    /// Loads a chunk from storage.
    pub async fn load_chunk(
        &self,
        pos: ChunkPos,
        min_y: i32,
        height: i32,
        level: Weak<World>,
    ) -> io::Result<Option<(ChunkAccess, ChunkStatus)>> {
        if let Ok(true) = self.chunk_exists(pos).await {
            if let Some(storage) = self.saved_chunks.read().await.get(&pos) {
                Ok(Some((
                    ChunkStorage::persistent_to_chunk(
                        &storage.prepared.persistent,
                        pos,
                        storage.chunk_status,
                        min_y,
                        height,
                        level,
                    ),
                    storage.chunk_status,
                )))
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Saves prepared chunk data to storage.
    pub async fn save_chunk_data(
        &self,
        prepared: PreparedChunkSave,
        status: ChunkStatus,
    ) -> io::Result<bool> {
        // Just track that this chunk has been saved
        // The actual data is in the live World/ChunkAccess, not persisted
        self.saved_chunks.write().await.insert(
            prepared.pos,
            SimpleRAMChunk {
                prepared,
                chunk_status: status,
            },
        );
        Ok(true)
    }

    /// Checks if a chunk exists in storage.
    pub async fn chunk_exists(&self, pos: ChunkPos) -> io::Result<bool> {
        Ok(self.saved_chunks.read().await.contains_key(&pos))
    }
}
