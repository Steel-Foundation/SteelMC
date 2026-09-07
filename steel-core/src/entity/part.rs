//! Multipart entity support.
//!
//! A multipart entity is one whose hitbox is split across several sub-entities, so
//! that different regions can take different damage. The Ender Dragon is the only
//! vanilla example.
//!
//! This version of vanilla has no shared base for those sub-entities:
//! `EnderDragonPart extends Entity` directly, and every engine site that has to
//! recognize one does so with `instanceof EnderDragonPart`. Steel instead follows
//! `NeoForge`'s `PartEntity` shape, so world queries, damage redirection and the
//! entity tracker never name a specific mob and a third-party multipart mob works
//! without engine changes.
//!
//! Parts are deliberately **not** registered with the world's entity manager, which
//! matches vanilla keeping them only in `ServerLevel.dragonParts`: they are never
//! sectioned, never saved, and never sent to clients. The client synthesizes them
//! from the parent's network ID; see [`crate::entity::EntityIdBlock`].

use std::sync::{Arc, OnceLock, Weak};

use glam::DVec3;
use steel_registry::entity_type::EntityDimensions;

use super::{Entity, SharedEntity, WeakEntity};

/// A sub-entity owned by a multipart parent.
///
/// Mirrors vanilla `EnderDragonPart`, generalized so engine code can support any
/// multipart mob. Implementors supply [`PartEntityBase`] through
/// [`Self::part_base`] and get the rest for free.
pub trait PartEntity: Entity {
    /// Returns this part's shared vanilla state.
    fn part_base(&self) -> &PartEntityBase;

    /// Returns the entity this part belongs to.
    ///
    /// Mirrors vanilla `EnderDragonPart.parentMob`. `None` only before the owning
    /// `Arc` has been bound, or once the parent has been dropped.
    fn parent(&self) -> Option<SharedEntity> {
        self.part_base().parent()
    }

    /// Returns this part's vanilla name, such as `"head"`, `"wing"` or `"tail"`.
    ///
    /// Mirrors vanilla `EnderDragonPart.name`. Parts of the same kind share a name.
    fn part_name(&self) -> &'static str {
        self.part_base().name()
    }

    /// Moves this part to `position`.
    ///
    /// Mirrors vanilla `EnderDragon.tickPart`. Parts live outside the world entity
    /// manager, so this bypasses the manager-commit path that a tracked entity's
    /// movement goes through.
    fn set_part_position(&self, position: DVec3) {
        self.base().set_position_local(position);
    }
}

/// Shared state for a [`PartEntity`].
///
/// Mirrors the `parentMob`, `name` and `size` fields of vanilla `EnderDragonPart`.
#[derive(Debug)]
pub struct PartEntityBase {
    /// The owning entity.
    ///
    /// Vanilla passes `this` from the parent's constructor. Rust cannot hand out a
    /// reference to a value that is still being constructed, so the world binds this
    /// once the parent's `Arc` exists and it starts being tracked.
    parent: OnceLock<WeakEntity>,
    name: &'static str,
    size: EntityDimensions,
}

impl PartEntityBase {
    /// Creates part state with the vanilla name and hitbox size.
    #[must_use]
    pub const fn new(name: &'static str, size: EntityDimensions) -> Self {
        Self {
            parent: OnceLock::new(),
            name,
            size,
        }
    }

    /// Returns the owning entity, if it is still alive and already bound.
    #[must_use]
    pub fn parent(&self) -> Option<SharedEntity> {
        self.parent.get().and_then(Weak::upgrade)
    }

    /// Returns the part's vanilla name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Returns the part's hitbox size.
    ///
    /// Mirrors vanilla `EnderDragonPart.getDimensions`, which ignores the pose.
    #[must_use]
    pub const fn size(&self) -> EntityDimensions {
        self.size
    }

    /// Binds the owning entity.
    ///
    /// Idempotent: later calls are ignored, so re-tracking a parent cannot rebind
    /// its parts to a different owner.
    pub fn bind_parent(&self, parent: &SharedEntity) {
        let _ = self.parent.set(Arc::downgrade(parent));
    }
}
