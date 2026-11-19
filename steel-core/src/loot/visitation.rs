use std::sync::Arc;

use steel_utils::{Identifier, locks::SyncMutex};

/// Owned recursion bookkeeping permits a suspended Rust frame to clean up when
/// its future is dropped, without borrowing the rest of its loot context.
#[derive(Default)]
pub(super) struct LootVisitation {
    tables: Arc<SyncMutex<Vec<Identifier>>>,
}

impl LootVisitation {
    pub(super) fn enter(&self, key: &Identifier) -> Option<LootVisit> {
        let mut tables = self.tables.lock();
        if tables.contains(key) {
            return None;
        }
        tables.push(key.clone());
        Some(LootVisit {
            tables: Arc::clone(&self.tables),
            depth: tables.len(),
        })
    }

    pub(super) fn current(&self) -> Option<Identifier> {
        self.tables.lock().last().cloned()
    }
}

pub(super) struct LootVisit {
    tables: Arc<SyncMutex<Vec<Identifier>>>,
    depth: usize,
}

impl Drop for LootVisit {
    fn drop(&mut self) {
        let mut tables = self.tables.lock();
        assert_eq!(
            tables.len(),
            self.depth,
            "loot futures must unwind their nested table frames in order"
        );
        tables.pop();
    }
}
