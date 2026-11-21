use std::sync::Arc;

use steel_utils::locks::AsyncRwLock;
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};

/// Admission permits serialize saves against complete loot operations. The gate
/// contains no game data; waiting jobs never lock a world, entity, or inventory.
#[derive(Default)]
pub(crate) struct LootLifecycle {
    gate: Arc<AsyncRwLock<()>>,
}

impl LootLifecycle {
    pub(crate) async fn enter(&self) -> OwnedRwLockReadGuard<()> {
        Arc::clone(&self.gate).read_owned().await
    }

    pub(crate) async fn save_barrier(&self) -> OwnedRwLockWriteGuard<()> {
        Arc::clone(&self.gate).write_owned().await
    }

    /// Tick callbacks defer snapshotting instead of waiting for loot or a save.
    pub(crate) fn try_save_barrier(&self) -> Option<OwnedRwLockWriteGuard<()>> {
        Arc::clone(&self.gate).try_write_owned().ok()
    }
}
