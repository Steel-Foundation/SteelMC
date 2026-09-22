use std::ops::Deref;
use std::sync::LazyLock;

use rustc_hash::FxHashMap;
use steel_utils::locks::SyncMutex;

use super::{COLLECTION_GATE, EntityArc, EntityWeak};
use crate::entity::{Entity, EntityGeneration};

pub(crate) struct OwnedTarget {
    pub(crate) entity: EntityWeak<dyn Entity>,
    pub(crate) count: usize,
}

pub(crate) type OwnedReferences =
    FxHashMap<EntityGeneration, FxHashMap<EntityGeneration, OwnedTarget>>;

pub(super) static OWNED_REFERENCES: LazyLock<SyncMutex<OwnedReferences>> =
    LazyLock::new(SyncMutex::default);

/// A strong entity reference in state visited by [`super::EntityOwnedState`].
///
/// Outside that container's locked storage this counts as an independent owner.
/// Cloning the dereferenced `EntityArc` always creates an independent handle.
pub struct EntityReference<T: Entity + ?Sized> {
    entity: EntityArc<T>,
    generation: EntityGeneration,
    erased: EntityWeak<dyn Entity>,
    owner: Option<EntityGeneration>,
}

impl<T: Entity + ?Sized> EntityReference<T> {
    /// Retains an exact target, initially without a containing entity.
    #[must_use]
    pub fn new(entity: EntityArc<T>) -> Self {
        let generation = entity.generation();
        // Type erasure requires an owned handle; only its weak reference is stored.
        let shared = EntityArc::clone(&entity).into_shared_entity();
        let erased = EntityArc::downgrade(&shared);
        Self {
            entity,
            generation,
            erased,
            owner: None,
        }
    }

    fn set_owner(&mut self, owner: Option<EntityGeneration>) {
        if self.owner == owner {
            return;
        }
        let _gate = COLLECTION_GATE.read();
        let mut references = OWNED_REFERENCES.lock();
        if let Some(previous) = self.owner {
            let Some(targets) = references.get_mut(&previous) else {
                panic!("owned entity reference is missing its owner");
            };
            let Some(target) = targets.get_mut(&self.generation) else {
                panic!("owned entity reference is missing its target");
            };
            target.count -= 1;
            if target.count == 0 {
                targets.remove(&self.generation);
            }
            if targets.is_empty() {
                references.remove(&previous);
            }
        }
        if let Some(owner) = owner {
            let target = references
                .entry(owner)
                .or_default()
                .entry(self.generation)
                .or_insert_with(|| OwnedTarget {
                    entity: self.erased.clone(),
                    count: 0,
                });
            target.count += 1;
        }
        self.owner = owner;
    }
}

impl<T: Entity + ?Sized> Deref for EntityReference<T> {
    type Target = EntityArc<T>;

    fn deref(&self) -> &Self::Target {
        &self.entity
    }
}

impl<T: Entity + ?Sized> Drop for EntityReference<T> {
    fn drop(&mut self) {
        // The entity handle is dropped only after both ownership locks are released.
        self.set_owner(None);
    }
}

/// Exposes entity fields to ownership tracking.
pub trait VisitEntityReferences {
    /// Visits each stored strong reference once, including nested containers.
    fn visit_entity_references(&mut self, visitor: &mut EntityReferenceVisitor);
}

/// Applies the container's current ownership to its strong entity fields.
pub struct EntityReferenceVisitor {
    pub(super) owner: Option<EntityGeneration>,
}

impl EntityReferenceVisitor {
    /// Updates ownership for an optional strong entity field.
    pub fn visit<T: Entity + ?Sized>(&mut self, reference: &mut Option<EntityReference<T>>) {
        if let Some(reference) = reference {
            reference.set_owner(self.owner);
        }
    }
}

impl super::EntityCollection {
    #[expect(
        clippy::unused_self,
        reason = "the receiver proves ownership changes are blocked by collection"
    )]
    pub(crate) fn owned_references(&self) -> parking_lot::MutexGuard<'_, OwnedReferences> {
        OWNED_REFERENCES.lock()
    }

    /// Drop the returned temporary handle only after leaving collection.
    pub(crate) fn retire_weak(
        &self,
        entity: &EntityWeak<dyn Entity>,
    ) -> Option<EntityArc<dyn Entity>> {
        let entity = EntityArc(entity.0.upgrade()?);
        self.retire(&entity);
        Some(entity)
    }
}
