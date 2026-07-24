//! Chunk-owned game-event listener bindings for block entities.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use steel_utils::{BlockPos, SectionPos, locks::SyncMutex};

use crate::block_entity::SharedBlockEntity;
use crate::world::game_event_listener::{
    GameEventListenerCount, GameEventListenerStorage, SharedGameEventListener,
};

struct BlockEntityListenerBinding {
    owner: SharedBlockEntity,
    listener: Option<SharedGameEventListener>,
}

/// Result of committing a listener selected without chunk or binding locks.
pub(crate) enum ListenerSelectionCommit {
    /// This call installed the selection.
    Committed,
    /// The same block entity was already selected by a reentrant call.
    AlreadySelected,
    /// A different owner was installed while the selection callback ran.
    Occupied,
}

/// Game-event registries and exact block-entity bindings owned by one `LevelChunk`.
///
/// The registry remains attached to a retained chunk while it is temporarily below Full. World
/// dispatch gates access through the active Full holder, matching Vanilla without reordering
/// listeners on revival.
pub(crate) struct LevelChunkGameEventListeners {
    pub(crate) registry: Arc<GameEventListenerStorage>,
    block_entities: SyncMutex<FxHashMap<BlockPos, BlockEntityListenerBinding>>,
}

impl LevelChunkGameEventListeners {
    #[must_use]
    pub(crate) fn new(listener_count: Arc<GameEventListenerCount>) -> Self {
        Self {
            registry: Arc::new(GameEventListenerStorage::with_count(listener_count)),
            block_entities: SyncMutex::new(FxHashMap::default()),
        }
    }

    /// Removes the stored selection unless `current` is still its exact storage owner.
    pub(crate) fn remove_obsolete(&self, pos: BlockPos, current: Option<&SharedBlockEntity>) {
        let binding = {
            let mut bindings = self.block_entities.lock();
            let keep = bindings.get(&pos).is_some_and(|binding| {
                current.is_some_and(|current| Arc::ptr_eq(&binding.owner, current))
            });
            if keep {
                return;
            }

            bindings.remove(&pos)
        };
        let Some(binding) = binding else { return };
        if let Some(listener) = binding.listener {
            let section_y = SectionPos::block_to_section_coord(pos.y());
            self.registry.unregister(section_y, &listener);
        }
    }

    /// Returns whether this exact entity already has a stored listener selection.
    #[must_use]
    pub(crate) fn is_selected(&self, owner: &SharedBlockEntity) -> bool {
        self.block_entities
            .lock()
            .get(&owner.get_block_pos())
            .is_some_and(|binding| Arc::ptr_eq(&binding.owner, owner))
    }

    /// Installs a selection only if the position stayed vacant while the provider ran.
    pub(crate) fn commit_selection(
        &self,
        owner: SharedBlockEntity,
        listener: Option<SharedGameEventListener>,
    ) -> ListenerSelectionCommit {
        let pos = owner.get_block_pos();
        let mut bindings = self.block_entities.lock();
        if let Some(binding) = bindings.get(&pos) {
            return if Arc::ptr_eq(&binding.owner, &owner) {
                ListenerSelectionCommit::AlreadySelected
            } else {
                ListenerSelectionCommit::Occupied
            };
        }

        if let Some(listener) = &listener {
            let section_y = SectionPos::block_to_section_coord(pos.y());
            self.registry.register(section_y, Arc::clone(listener));
        }
        bindings.insert(pos, BlockEntityListenerBinding { owner, listener });
        ListenerSelectionCommit::Committed
    }

    /// Returns positions with retained selections, including providers that returned no listener.
    #[must_use]
    pub(crate) fn block_entity_positions(&self) -> Vec<BlockPos> {
        self.block_entities.lock().keys().copied().collect()
    }
}
