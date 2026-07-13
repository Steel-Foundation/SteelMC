//! Safe entity downcasting via the steel entity registry.
//!
//! Also provides [`EntityIdentifier`], a string-based identity trait that the
//! `#[entity_behavior]` macro implements automatically from the `class` attribute.
//!
//! Each registered [`EntityTypeRef`] has a unique numeric ID. Comparing those IDs is a sound
//! proxy for type identity — no extra per-entity trait or boilerplate required.
//!
//! # Usage
//! ```ignore
//! // Hold the lock for multiple operations:
//! let mut guard = base.lock_entity();
//! if let Some(pig) = guard.downcast::<Pig>() {
//!     println!("age = {}", pig.get_age());
//! }
//!
//! // Closure form (lock acquired and released automatically):
//! base.with_entity_as::<Pig, _>(|pig| pig.is_baby());
//! ```
//!
//! [`RawEntity`] is a many-to-one fallback and is only accessed via `&dyn Entity`.

use std::ops::{Deref, DerefMut};

use parking_lot::{ArcMutexGuard, RawMutex};
use steel_utils::Identifier;
use steel_utils::locks::SyncMutexGuard;

use super::Entity;
use crate::player::Player;

/// Downcast to `&mut T`. Returns `None` if not attached or wrong kind.
///
/// `kind` must be the `EntityTypeRef` that corresponds to `T`.
pub fn downcast_entity<T: EntityIdentifier>(entity: &mut dyn Entity) -> Option<&mut T> {
    if entity.entity_type().key == T::KEY {
        // SAFETY: id equality proves the concrete type behind the fat pointer is T.
        Some(unsafe { &mut *(entity as *mut dyn Entity as *mut T) })
    } else {
        None
    }
}

/// String-based identity for entity structs.
///
/// Implemented automatically by `#[entity_behavior(class = "...")]`. The `ID` constant
/// holds the class string from the attribute, making it usable for downcast checks and
/// addon extensibility without needing an `EntityTypeRef`:
///
/// ```rust,ignore
/// if entity.entity_id() == ChestMinecartEntity::ID { ... }
/// ```
pub trait EntityIdentifier {
    /// The vanilla Identifier used for downcasting validation
    const KEY: Identifier;
}

/// RAII guard over the entity mutex that exposes typed downcast.
///
/// Obtained via [`EntityBase::lock_entity`]. Holds the lock until dropped, so all downcast
/// references live within this guard's lifetime.
///
/// A player is a normal entity too: its behavior lives behind the player mutex rather than
/// the `entity` cell, so locking one yields an owning [`ArcMutexGuard`] over the `Player`
/// (which implements [`Entity`]). Callers see `&mut dyn Entity` either way.
pub enum LockedEntity<'a> {
    /// A non-player entity locked through the base's `entity` cell.
    Borrowed(SyncMutexGuard<'a, dyn Entity + 'static>),
    /// A player locked through its shared `Arc<SyncMutex<Player>>`.
    Player(ArcMutexGuard<RawMutex, Player>),
}

impl<'a> LockedEntity<'a> {
    /// Returns a shared reference to the inner entity, if attached.
    pub fn get(&self) -> &dyn Entity {
        match self {
            LockedEntity::Borrowed(guard) => guard.deref(),
            LockedEntity::Player(guard) => &**guard,
        }
    }

    /// Returns a mutable reference to the inner entity, if attached.
    pub fn get_mut(&mut self) -> &mut dyn Entity {
        match self {
            LockedEntity::Borrowed(guard) => guard.deref_mut(),
            LockedEntity::Player(guard) => &mut **guard,
        }
    }

    /// Downcast to `&mut T`. Returns `None` if not attached or wrong kind.
    ///
    /// `kind` must be the `EntityTypeRef` that corresponds to `T`.
    pub fn downcast<T: EntityIdentifier>(&mut self) -> Option<&mut T> {
        let entity: &mut dyn Entity = self.get_mut();
        downcast_entity(entity)
    }

    /// Downcast to `&mut T`. Returns `None` if not attached or wrong kind.
    ///
    /// `kind` must be the `EntityTypeRef` that corresponds to `T`.
    pub unsafe fn downcast_unchecked<T>(&mut self) -> &mut T {
        let entity: &mut dyn Entity = self.get_mut();
        unsafe { &mut *(entity as *mut dyn Entity as *mut T) }
    }
}
