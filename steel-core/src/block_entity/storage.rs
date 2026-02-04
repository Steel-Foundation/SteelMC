//! Block entity storage for chunks.

use rustc_hash::FxHashMap;
use steel_utils::{BlockPos, locks::SyncMutex, locks::SyncRwLock};

use super::SharedBlockEntity;

/// Storage for block entities in a chunk.
///
/// Encapsulates both the main storage map and the ticking list to ensure
/// they stay in sync.
pub struct BlockEntityStorage {
    /// Block entities keyed by their position.
    entities: SyncRwLock<FxHashMap<BlockPos, SharedBlockEntity>>,
    /// Block entities that need to be ticked every game tick.
    /// This is a subset of `entities` for efficient iteration.
    tickers: SyncMutex<Vec<SharedBlockEntity>>,
}

impl BlockEntityStorage {
    /// Creates a new empty block entity storage.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entities: SyncRwLock::new(FxHashMap::default()),
            tickers: SyncMutex::new(Vec::new()),
        }
    }

    /// Gets a block entity at the given position.
    #[must_use]
    pub fn get(&self, pos: BlockPos) -> Option<SharedBlockEntity> {
        self.entities.read().get(&pos).cloned()
    }

    /// Returns all block entities in this storage.
    #[must_use]
    pub fn get_all(&self) -> Vec<SharedBlockEntity> {
        self.entities.read().values().cloned().collect()
    }

    /// Returns the number of block entities in this storage.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entities.read().len()
    }

    /// Returns whether there are no block entities in this storage.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.read().is_empty()
    }

    /// Sets a block entity at its position.
    ///
    /// Replaces any existing block entity at the position (marking it as removed).
    /// Does NOT automatically register for ticking - use `add_and_register` instead.
    pub fn set(&self, block_entity: SharedBlockEntity) {
        let pos = block_entity.lock().get_block_pos();
        let mut entities = self.entities.write();

        // Remove old entity if present
        if let Some(old) = entities.remove(&pos) {
            old.lock().set_removed();
            self.remove_from_tickers(&pos);
        }

        block_entity.lock().clear_removed();
        entities.insert(pos, block_entity);
    }

    /// Removes a block entity at the given position.
    ///
    /// Marks the entity as removed and removes it from the ticking list.
    pub fn remove(&self, pos: BlockPos) {
        let mut entities = self.entities.write();
        if let Some(entity) = entities.remove(&pos) {
            entity.lock().set_removed();
        }
        drop(entities);
        self.remove_from_tickers(&pos);
    }

    /// Adds a block entity and registers it for ticking if needed.
    ///
    /// This is the main entry point for adding block entities.
    pub fn add_and_register(&self, block_entity: SharedBlockEntity) {
        let is_ticking = block_entity.lock().is_ticking();
        self.set(block_entity.clone());

        if is_ticking {
            self.tickers.lock().push(block_entity);
        }
    }

    /// Updates the ticking status of a block entity.
    ///
    /// Call this when a block entity's ticking status may have changed.
    pub fn update_ticker(&self, block_entity: &SharedBlockEntity) {
        let guard = block_entity.lock();
        let pos = guard.get_block_pos();
        let should_tick = guard.is_ticking();
        drop(guard);

        let mut tickers = self.tickers.lock();
        let already_ticking = tickers.iter().any(|e| e.lock().get_block_pos() == pos);

        if should_tick && !already_ticking {
            tickers.push(block_entity.clone());
        } else if !should_tick && already_ticking {
            tickers.retain(|e| e.lock().get_block_pos() != pos);
        }
    }

    /// Returns a copy of the ticking block entities for iteration.
    ///
    /// Filters out removed entities.
    #[must_use]
    pub fn get_tickers(&self) -> Vec<SharedBlockEntity> {
        self.tickers
            .lock()
            .iter()
            .filter(|e| !e.lock().is_removed())
            .cloned()
            .collect()
    }

    /// Cleans up removed entities from the ticking list.
    pub fn cleanup_tickers(&self) {
        self.tickers.lock().retain(|e| !e.lock().is_removed());
    }

    /// Clears all block entities.
    ///
    /// Marks all entities as removed.
    pub fn clear(&self) {
        let mut entities = self.entities.write();
        for entity in entities.values() {
            entity.lock().set_removed();
        }
        entities.clear();
        drop(entities);

        self.tickers.lock().clear();
    }

    /// Removes a block entity from the ticking list by position.
    fn remove_from_tickers(&self, pos: &BlockPos) {
        self.tickers
            .lock()
            .retain(|e| &e.lock().get_block_pos() != pos);
    }
}

impl Default for BlockEntityStorage {
    fn default() -> Self {
        Self::new()
    }
}
