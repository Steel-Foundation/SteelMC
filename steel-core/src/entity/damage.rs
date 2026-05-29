//! Damage source system.

use glam::DVec3;
use steel_registry::damage_type::{DamageScaling, DamageType};
use steel_registry::{REGISTRY, TaggedRegistryExt, vanilla_damage_type_tags};

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
        REGISTRY.damage_types.is_in_tag(
            self.damage_type,
            &vanilla_damage_type_tags::BYPASSES_INVULNERABILITY_TAG,
        )
    }

    /// Whether this damage belongs to vanilla's `is_fall` damage type tag.
    #[must_use]
    pub fn is_fall(&self) -> bool {
        REGISTRY
            .damage_types
            .is_in_tag(self.damage_type, &vanilla_damage_type_tags::IS_FALL_TAG)
    }

    /// Whether this damage bypasses the invulnerability cooldown timer.
    /// No vanilla damage types currently use this, but the logic exists in
    /// `LivingEntity.hurtServer()`.
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

#[cfg(test)]
mod tests {
    use steel_registry::{test_support::init_test_registry, vanilla_damage_types};

    use super::DamageSource;

    #[test]
    fn is_fall_uses_vanilla_damage_type_tag() {
        init_test_registry();

        assert!(DamageSource::environment(&vanilla_damage_types::FALL).is_fall());
        assert!(DamageSource::environment(&vanilla_damage_types::STALAGMITE).is_fall());
        assert!(DamageSource::environment(&vanilla_damage_types::ENDER_PEARL).is_fall());
        assert!(!DamageSource::environment(&vanilla_damage_types::DROWN).is_fall());
    }

    #[test]
    fn bypasses_invulnerability_uses_vanilla_damage_type_tag() {
        init_test_registry();

        assert!(
            DamageSource::environment(&vanilla_damage_types::OUT_OF_WORLD)
                .bypasses_invulnerability()
        );
        assert!(!DamageSource::environment(&vanilla_damage_types::FALL).bypasses_invulnerability());
    }
}
