//! Damage source system.

use steel_registry::damage_type::{DamageScaling, DamageType};
use steel_utils::math::Vector3;

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
    pub source_position: Option<Vector3<f64>>,
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
    /// TODO: use damage type tag query once `DamageTypeRegistry` supports tags
    #[must_use]
    pub fn bypasses_invulnerability(&self) -> bool {
        matches!(&*self.damage_type.key.path, "out_of_world" | "generic_kill")
    }

    /// Whether this damage bypasses the invulnerability cooldown timer.
    /// No vanilla damage types currently use this, but the logic exists in
    /// `LivingEntity.hurtServer()`.
    /// TODO: use damage type tag query once supported
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
