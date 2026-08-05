//! Damage source system.

use glam::DVec3;
use steel_registry::{
    REGISTRY, TaggedRegistryExt, damage_type::DamageScaling, damage_type::DamageType,
    vanilla_damage_type_tags,
};

use crate::entity::Entity;

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

    /// Adds the entity ultimately responsible for the damage.
    #[must_use]
    pub const fn with_causing_entity(mut self, entity_id: i32) -> Self {
        self.causing_entity_id = Some(entity_id);
        self
    }

    /// Adds the direct entity that delivered the damage.
    #[must_use]
    pub const fn with_direct_entity(mut self, entity_id: i32) -> Self {
        self.direct_entity_id = Some(entity_id);
        self
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
        self.causing_entity_id == self.direct_entity_id
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

    /// Whether this damage scales with world difficulty for the resolved causing entity.
    ///
    /// `causing_entity` is `None` when the source has no cause or its stored entity ID no
    /// longer resolves. Both cases fail Vanilla's living non-player type check.
    #[must_use]
    pub fn scales_with_difficulty(&self, causing_entity: Option<&dyn Entity>) -> bool {
        match self.damage_type.scaling {
            DamageScaling::Never => false,
            DamageScaling::WhenCausedByLivingNonPlayer => causing_entity.is_some_and(|entity| {
                entity.as_living_entity().is_some() && entity.as_player().is_none()
            }),
            DamageScaling::Always => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Weak;

    use glam::DVec3;
    use steel_registry::{
        test_support::init_test_registry, vanilla_damage_types, vanilla_entities,
    };

    use crate::entity::entities::{FireworkRocketEntity, PigEntity};

    use super::*;

    #[test]
    fn conditional_difficulty_scaling_requires_a_resolved_living_non_player() {
        init_test_registry();
        let source = DamageSource::environment(&vanilla_damage_types::FIREWORKS);
        let pig = PigEntity::new(&vanilla_entities::PIG, 1, DVec3::ZERO, Weak::new());
        let rocket = FireworkRocketEntity::new(
            &vanilla_entities::FIREWORK_ROCKET,
            2,
            DVec3::ZERO,
            Weak::new(),
        );

        assert!(source.scales_with_difficulty(Some(&pig)));
        assert!(!source.scales_with_difficulty(Some(&rocket)));
        assert!(!source.scales_with_difficulty(None));
    }
}
