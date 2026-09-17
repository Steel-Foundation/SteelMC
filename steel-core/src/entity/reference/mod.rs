//! Entity ownership with weak promotion coordinated against history collection.

use std::fmt;
use std::marker::Unsize;
use std::ops::{CoerceUnsized, Deref, DispatchFromDyn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use steel_utils::locks::SyncRwLock;

mod living;
pub use living::LivingEntityRef;

static COLLECTION_GATE: SyncRwLock<()> = SyncRwLock::new(());

struct EntityAllocation<T: ?Sized> {
    retired: AtomicBool,
    value: T,
}

/// Strong ownership of an exact entity allocation, independent of world membership.
///
/// Coerces from a concrete entity to `dyn Entity`, like `Arc`. Raw `Arc` conversion
/// is deliberately unavailable: it would bypass coordinated weak promotion.
pub struct EntityArc<T: ?Sized>(Arc<EntityAllocation<T>>);

/// A non-owning entity reference that cannot revive a collected allocation.
pub struct EntityWeak<T: ?Sized>(Weak<EntityAllocation<T>>);

impl<T> EntityArc<T> {
    /// Allocates an entity with independent shared ownership.
    pub fn new(value: T) -> Self {
        Self(Arc::new(EntityAllocation {
            retired: AtomicBool::new(false),
            value,
        }))
    }
}

impl<T: ?Sized> EntityArc<T> {
    /// Creates a non-owning reference to this allocation.
    #[must_use]
    pub fn downgrade(this: &Self) -> EntityWeak<T> {
        EntityWeak(Arc::downgrade(&this.0))
    }

    /// Tests allocation identity, ignoring trait-object metadata.
    #[must_use]
    pub fn ptr_eq(this: &Self, other: &Self) -> bool {
        Arc::ptr_eq(&this.0, &other.0)
    }
}

impl<T: ?Sized> Clone for EntityArc<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T: ?Sized> Deref for EntityArc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0.value
    }
}

impl<T: ?Sized> AsRef<T> for EntityArc<T> {
    fn as_ref(&self) -> &T {
        self
    }
}

impl<T: fmt::Debug + ?Sized> fmt::Debug for EntityArc<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.value.fmt(formatter)
    }
}

impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<EntityArc<U>> for EntityArc<T> {}
impl<T: ?Sized + Unsize<U>, U: ?Sized> DispatchFromDyn<EntityArc<U>> for EntityArc<T> {}

impl<T> EntityWeak<T> {
    /// Creates an empty weak reference.
    #[must_use]
    pub const fn new() -> Self {
        Self(Weak::new())
    }
}

impl<T> Default for EntityWeak<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ?Sized> EntityWeak<T> {
    /// Pruning hint only: retired allocations may still have strong references.
    /// Unlike `upgrade`, this cannot run an entity destructor.
    pub(crate) fn strong_count(&self) -> usize {
        self.0.strong_count()
    }

    /// Retains the entity if it has neither been destroyed nor collected.
    #[must_use]
    pub fn upgrade(&self) -> Option<EntityArc<T>> {
        let (allocation, retired) = {
            let _gate = COLLECTION_GATE.read();
            let allocation = self.0.upgrade()?;
            let retired = allocation.retired.load(Ordering::Relaxed);
            (allocation, retired)
        };
        // The last temporary reference may run an entity destructor. Drop it
        // after unlocking, including when retirement prevented the promotion.
        if retired {
            return None;
        }
        Some(EntityArc(allocation))
    }

    /// Tests allocation identity, including after the entity has been destroyed.
    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.0, &other.0)
    }
}

impl<T: ?Sized> Clone for EntityWeak<T> {
    fn clone(&self) -> Self {
        Self(Weak::clone(&self.0))
    }
}

impl<T: ?Sized> fmt::Debug for EntityWeak<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl<T: ?Sized + Unsize<U>, U: ?Sized> CoerceUnsized<EntityWeak<U>> for EntityWeak<T> {}

/// Authority to inspect ownership and retire allocations while weak promotion is stopped.
pub(super) struct EntityCollection {
    _private: (),
}

impl EntityCollection {
    /// The callback must not call entity code or drop entities under the gate.
    /// Return collected records and release them after this method returns.
    pub(super) fn run<R>(collect: impl FnOnce(&Self) -> R) -> R {
        let _gate = COLLECTION_GATE.write();
        collect(&Self { _private: () })
    }

    #[expect(
        clippy::unused_self,
        reason = "the receiver proves weak promotion is blocked by the collection gate"
    )]
    pub(super) fn strong_count<T: ?Sized>(&self, entity: &EntityArc<T>) -> usize {
        Arc::strong_count(&entity.0)
    }

    /// Call only after proving that history owns every strong reference and no
    /// independently owned victim can reach this allocation through its history.
    #[expect(
        clippy::unused_self,
        reason = "the receiver proves weak promotion is blocked by the collection gate"
    )]
    pub(super) fn retire<T: ?Sized>(&self, entity: &EntityArc<T>) {
        entity.0.retired.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::mpsc, thread};

    use super::{EntityArc, EntityCollection};

    #[test]
    fn retirement_blocks_a_waiting_promotion_before_references_are_dropped() {
        let retained = EntityArc::new(17_u32);
        let weak = EntityArc::downgrade(&retained);
        let (started, ready) = mpsc::channel();
        let promotion = EntityCollection::run(|collection| {
            let promotion = thread::spawn(move || {
                started.send(()).expect("announce promotion");
                weak.upgrade()
            });
            ready.recv().expect("promotion thread started");
            collection.retire(&retained);
            promotion
        });
        assert!(promotion.join().expect("promotion thread").is_none());
        drop(retained);
    }
}
