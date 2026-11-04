//! Multipart entities, shaped after `NeoForge`'s `PartEntity`.
//!
//! Vanilla has no shared base for `EnderDragonPart`, so engine code going through
//! [`PartEntity`] never names a specific mob. Like vanilla's `ServerLevel.dragonParts`,
//! parts stay out of the entity manager: they are never sectioned, saved or sent to
//! clients, which synthesize them from the parent's ID (see
//! [`crate::entity::EntityIdBlock`]).

use std::sync::{Arc, OnceLock, Weak};

use glam::DVec3;
use steel_registry::entity_type::EntityDimensions;

use super::{Entity, SharedEntity, WeakEntity};

/// A sub-entity of a multipart parent, generalizing vanilla `EnderDragonPart`.
pub trait PartEntity: Entity {
    /// Returns this part's shared vanilla state.
    fn part_base(&self) -> &PartEntityBase;

    /// Returns the owning entity; `None` before it is bound or after it is dropped.
    fn parent(&self) -> Option<SharedEntity> {
        self.part_base().parent()
    }

    /// Returns this part's vanilla name, such as `"head"` or `"tail"`, shared by parts of one kind.
    fn part_name(&self) -> &'static str {
        self.part_base().name()
    }

    /// Moves this part. Parts are outside the entity manager, so this skips its movement commit.
    fn set_part_position(&self, position: DVec3) {
        self.base().set_position_local(position);
    }
}

/// Shared state for a [`PartEntity`]: `EnderDragonPart`'s `parentMob`, `name` and `size`.
#[derive(Debug)]
pub struct PartEntityBase {
    /// Bound by the world once the parent's `Arc` exists, since Rust cannot pass `this`
    /// from the parent's constructor as vanilla does.
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

    /// Returns the part's hitbox size, which ignores the pose as in vanilla.
    #[must_use]
    pub const fn size(&self) -> EntityDimensions {
        self.size
    }

    /// Binds the owning entity. Later calls are ignored, so a re-tracked parent cannot
    /// rebind its parts.
    pub fn bind_parent(&self, parent: &SharedEntity) {
        let _ = self.parent.set(Arc::downgrade(parent));
    }
}
