use std::sync::Arc;

use glam::DVec3;
use steel_registry::{REGISTRY, TaggedRegistryExt, damage_type::DamageType};
use steel_utils::Identifier;

use super::DamageSource;
use crate::entity::{EntityGeneration, SharedEntity, WeakEntity};

struct WeakSourceEntity {
    entity: WeakEntity,
    generation: EntityGeneration,
}

impl WeakSourceEntity {
    fn new(entity: &SharedEntity) -> Self {
        Self {
            entity: Arc::downgrade(entity),
            generation: entity.generation(),
        }
    }

    fn resolve(&self) -> ResolvedSourceEntity {
        ResolvedSourceEntity {
            entity: self.entity.upgrade(),
            generation: self.generation,
        }
    }
}

pub(super) struct WeakDamageSource {
    damage_type: &'static DamageType,
    causing_entity: Option<WeakSourceEntity>,
    direct_entity: Option<WeakSourceEntity>,
    source_position: Option<DVec3>,
}

impl WeakDamageSource {
    pub(super) fn new(source: &DamageSource) -> Self {
        Self {
            damage_type: source.damage_type,
            causing_entity: source.causing_entity().map(WeakSourceEntity::new),
            direct_entity: source.direct_entity().map(WeakSourceEntity::new),
            source_position: source.source_position_raw(),
        }
    }

    pub(super) fn resolve(&self) -> RecentDamageSource {
        RecentDamageSource {
            damage_type: self.damage_type,
            causing_entity: self.causing_entity.as_ref().map(WeakSourceEntity::resolve),
            direct_entity: self.direct_entity.as_ref().map(WeakSourceEntity::resolve),
            source_position: self.source_position,
        }
    }
}

struct ResolvedSourceEntity {
    entity: Option<SharedEntity>,
    generation: EntityGeneration,
}

/// A read of recent history, retaining the original entities that still exist.
///
/// Unlike vanilla, a removed or unmanaged victim's history does not keep source
/// entities alive. Generation accessors distinguish a released entity from an
/// originally absent one. Each entity resolves independently, so a released
/// projectile does not hide its still-live shooter.
pub struct RecentDamageSource {
    /// The original damage type, even when its entities have been released.
    pub damage_type: &'static DamageType,
    causing_entity: Option<ResolvedSourceEntity>,
    direct_entity: Option<ResolvedSourceEntity>,
    source_position: Option<DVec3>,
}

impl RecentDamageSource {
    /// The original causing entity, if it was still alive when history was read.
    #[must_use]
    pub fn causing_entity(&self) -> Option<&SharedEntity> {
        self.causing_entity.as_ref()?.entity.as_ref()
    }

    /// The original direct entity, if it was still alive when history was read.
    #[must_use]
    pub fn direct_entity(&self) -> Option<&SharedEntity> {
        self.direct_entity.as_ref()?.entity.as_ref()
    }

    /// Original allocation identity; `None` means the source never had a cause.
    #[must_use]
    pub fn causing_entity_generation(&self) -> Option<EntityGeneration> {
        self.causing_entity.as_ref().map(|source| source.generation)
    }

    /// Original allocation identity; `None` means the source never had a direct entity.
    #[must_use]
    pub fn direct_entity_generation(&self) -> Option<EntityGeneration> {
        self.direct_entity.as_ref().map(|source| source.generation)
    }

    /// Original directness, including after either entity has been released.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        self.causing_entity_generation() == self.direct_entity_generation()
    }

    /// Tests the original damage type's tag membership.
    #[must_use]
    pub fn is(&self, tag: &Identifier) -> bool {
        REGISTRY.damage_types.is_in_tag(self.damage_type, tag)
    }

    /// Explicit position, or the retained direct entity's current position.
    #[must_use]
    pub fn source_position(&self) -> Option<DVec3> {
        self.source_position
            .or_else(|| self.direct_entity().map(|entity| entity.position()))
    }

    /// The original explicit source position, without substituting an entity position.
    #[must_use]
    pub const fn source_position_raw(&self) -> Option<DVec3> {
        self.source_position
    }
}
