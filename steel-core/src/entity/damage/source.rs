use crate::player::Player;
use std::{fmt, sync::Arc};

use glam::DVec3;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, damage_type::DamageScaling, damage_type::DamageType,
    vanilla_damage_type_tags,
};

use crate::entity::SharedEntity;

/// Describes damage while retaining the exact direct and causing entity allocations.
///
/// Mirrors vanilla's strong entity references: removal and respawn never rebind
/// attribution. Store recent damage in [`super::DamageHistory`] rather than on
/// entities, where self or mutual damage would create reference cycles.
#[derive(Clone)]
pub struct DamageSource {
    /// The damage type registry entry.
    pub damage_type: &'static DamageType,
    causing_entity: Option<SharedEntity>,
    direct_entity: Option<SharedEntity>,
    /// Explicit source position, distinct from the direct entity's current position.
    source_position: Option<DVec3>,
}

impl DamageSource {
    /// Environmental damage with no entity or position context (void, starvation, etc.).
    #[must_use]
    pub const fn environment(damage_type: &'static DamageType) -> Self {
        Self {
            damage_type,
            causing_entity: None,
            direct_entity: None,
            source_position: None,
        }
    }

    /// Adds the entity ultimately responsible for the damage.
    #[must_use]
    pub fn with_causing_entity(mut self, entity: SharedEntity) -> Self {
        self.causing_entity = Some(entity);
        self
    }

    /// Adds the direct entity that delivered the damage.
    #[must_use]
    pub fn with_direct_entity(mut self, entity: SharedEntity) -> Self {
        self.direct_entity = Some(entity);
        self
    }

    /// The original cause (e.g. a projectile's shooter), retained after removal.
    #[must_use]
    pub const fn causing_entity(&self) -> Option<&SharedEntity> {
        self.causing_entity.as_ref()
    }

    /// The original direct entity (e.g. the projectile), retained after removal.
    #[must_use]
    pub const fn direct_entity(&self) -> Option<&SharedEntity> {
        self.direct_entity.as_ref()
    }

    /// Vanilla `getSourcePosition`, distinct from the raw position sent in packets.
    #[must_use]
    pub fn source_position(&self) -> Option<DVec3> {
        self.source_position
            .or_else(|| self.direct_entity().map(|entity| entity.position()))
    }

    /// Vanilla `sourcePositionRaw`, used by the damage-event packet.
    #[must_use]
    pub const fn source_position_raw(&self) -> Option<DVec3> {
        self.source_position
    }

    /// Whether the causing player has vanilla infinite-materials abilities.
    #[must_use]
    pub fn is_creative_player(&self) -> bool {
        self.causing_entity()
            .and_then(|entity| entity.as_player())
            .is_some_and(Player::has_infinite_materials)
    }

    /// Adds the vanilla source position used by damage events and knockback.
    #[must_use]
    pub const fn with_source_position(mut self, source_position: DVec3) -> Self {
        self.source_position = Some(source_position);
        self
    }

    /// Whether this damage bypasses creative/spectator invulnerability.
    #[must_use]
    pub fn bypasses_invulnerability(&self) -> bool {
        self.is(&vanilla_damage_type_tags::DamageTypeTag::BYPASSES_INVULNERABILITY)
    }

    /// Returns whether this damage type is in the given vanilla damage-type tag.
    #[must_use]
    pub fn is(&self, tag: &steel_utils::Identifier) -> bool {
        REGISTRY.damage_types.is_in_tag(self.damage_type, tag)
    }

    /// Returns vanilla `DamageSource.isDirect`.
    #[must_use]
    pub fn is_direct(&self) -> bool {
        match (&self.causing_entity, &self.direct_entity) {
            (None, None) => true,
            (Some(cause), Some(direct)) => Arc::ptr_eq(cause, direct),
            _ => false,
        }
    }

    /// Whether this damage bypasses the invulnerability cooldown timer.
    /// No vanilla damage types currently use this, but the logic exists in
    /// `LivingEntity.hurtServer()`.
    /// TODO: use damage type tag query once supported
    #[expect(clippy::unused_self, reason = "this is an api function")]
    #[must_use]
    pub const fn bypasses_cooldown(&self) -> bool {
        false
    }

    /// Whether this damage scales with difficulty for the retained causing entity.
    #[must_use]
    pub fn scales_with_difficulty(&self) -> bool {
        let causing_entity = self.causing_entity.as_deref();
        match self.damage_type.scaling {
            DamageScaling::Never => false,
            DamageScaling::WhenCausedByLivingNonPlayer => causing_entity.is_some_and(|entity| {
                entity.as_living_entity().is_some() && entity.as_player().is_none()
            }),
            DamageScaling::Always => true,
        }
    }
}

impl fmt::Debug for DamageSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DamageSource")
            .field("damage_type", &self.damage_type)
            .field(
                "causing_entity",
                &self.causing_entity().map(|entity| entity.generation()),
            )
            .field(
                "direct_entity",
                &self.direct_entity().map(|entity| entity.generation()),
            )
            .field("source_position", &self.source_position)
            .finish()
    }
}
