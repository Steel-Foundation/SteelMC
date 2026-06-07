//! Damage source system.

use glam::DVec3;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, damage_type::DamageScaling, damage_type::DamageType,
    vanilla_damage_type_tags,
};

/// Describes how an entity was damaged.
#[derive(Debug, Clone)]
pub struct DamageSource {
    /// The damage type registry entry.
    pub damage_type: &'static DamageType,
    /// The entity ultimately responsible (e.g. the shooter for projectiles).
    pub causing_entity_id: Option<i32>,
    /// The entity that directly dealt the damage (e.g. the projectile itself).
    pub direct_entity_id: Option<i32>,
    /// Source position (for explosions, etc.).
    pub source_position: Option<DVec3>,
}

impl DamageSource {
    /// Environmental damage with no entity or position context (void, starvation, etc.).
    #[must_use]
    pub const fn environment(damage_type: &'static DamageType) -> Self {
        Self {
            damage_type,
            causing_entity_id: None,
            direct_entity_id: None,
            source_position: None,
        }
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

    /// Whether this damage bypasses the invulnerability cooldown timer.
    /// No vanilla damage types currently use this, but the logic exists in
    /// `LivingEntity.hurtServer()`.
    /// TODO: use damage type tag query once supported
    #[expect(clippy::unused_self, reason = "this is an api function")]
    #[must_use]
    pub const fn bypasses_cooldown(&self) -> bool {
        false
    }

    /// Whether this damage scales with world difficulty.
    /// Reads the `scaling` field from the damage type registry entry.
    #[must_use]
    pub const fn scales_with_difficulty(&self) -> bool {
        match self.damage_type.scaling {
            DamageScaling::Never => false,
            // TODO: WhenCausedByLivingNonPlayer needs entity type checking
            DamageScaling::Always | DamageScaling::WhenCausedByLivingNonPlayer => true,
        }
    }
}
