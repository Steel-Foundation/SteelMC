use std::fmt;
use std::ops::{Deref, DerefMut};

use parking_lot::MutexGuard;
use steel_utils::locks::SyncMutex;

use super::{EntityReferenceVisitor, VisitEntityReferences};
use crate::entity::{EntityBase, EntityGeneration};

/// Mutex-protected state whose visited references belong to its containing entity.
///
/// The state must live inside its owner. Borrowed or extracted references count
/// as independent owners. Visitors run outside the collection gate.
pub struct EntityOwnedState<T: VisitEntityReferences> {
    owner: EntityGeneration,
    state: SyncMutex<T>,
}

impl<T: VisitEntityReferences> EntityOwnedState<T> {
    /// Creates state to be stored inside `owner`'s entity allocation.
    pub fn new(owner: &EntityBase, mut state: T) -> Self {
        let owner = owner.generation();
        state.visit_entity_references(&mut EntityReferenceVisitor { owner: Some(owner) });
        Self {
            owner,
            state: SyncMutex::new(state),
        }
    }

    /// Borrows mutable state and temporarily makes its references independent.
    pub fn lock(&self) -> EntityOwnedStateGuard<'_, T> {
        let mut state = self.state.lock();
        state.visit_entity_references(&mut EntityReferenceVisitor { owner: None });
        EntityOwnedStateGuard {
            owner: self.owner,
            state,
        }
    }
}

impl<T: VisitEntityReferences> Drop for EntityOwnedState<T> {
    fn drop(&mut self) {
        self.state
            .get_mut()
            .visit_entity_references(&mut EntityReferenceVisitor { owner: None });
    }
}

impl<T: VisitEntityReferences + fmt::Debug> fmt::Debug for EntityOwnedState<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.state.fmt(formatter)
    }
}

/// A state guard that restores field ownership before releasing the mutex.
pub struct EntityOwnedStateGuard<'a, T: VisitEntityReferences> {
    owner: EntityGeneration,
    state: MutexGuard<'a, T>,
}

impl<T: VisitEntityReferences> Deref for EntityOwnedStateGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.state
    }
}

impl<T: VisitEntityReferences> DerefMut for EntityOwnedStateGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.state
    }
}

impl<T: VisitEntityReferences> Drop for EntityOwnedStateGuard<'_, T> {
    fn drop(&mut self) {
        self.state
            .visit_entity_references(&mut EntityReferenceVisitor {
                owner: Some(self.owner),
            });
    }
}
