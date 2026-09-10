//! Mob-effect behavior registry.

use std::ops::Deref;
use std::sync::OnceLock;

use steel_registry::mob_effect::MobEffectRef;
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};

use crate::entity::mob_effect::MobEffectBehavior;

/// Default vanilla `MobEffect` behavior for effects with no custom tick or
/// instantaneous logic — pure duration/amplifier/attribute-modifier data,
/// matching vanilla effects registered as plain `MobEffect` instances.
struct DefaultMobEffect;

impl MobEffectBehavior for DefaultMobEffect {}

/// Wrapper for the global mob-effect behavior registry that implements `Deref`.
pub struct MobEffectBehaviorLock(pub OnceLock<MobEffectBehaviorRegistry>);

impl Deref for MobEffectBehaviorLock {
    type Target = MobEffectBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0.get().expect("Mob effect behaviors not initialized")
    }
}

/// Global mob-effect behavior registry.
///
/// Access behaviors directly via deref: `MOB_EFFECT_BEHAVIORS.get_behavior(effect)`
pub static MOB_EFFECT_BEHAVIORS: MobEffectBehaviorLock = MobEffectBehaviorLock(OnceLock::new());

/// Registry for mob-effect behaviors.
///
/// Created after the main registry is frozen. All mob effects are
/// initialized with a default behavior matching a bare vanilla `MobEffect`
/// instance, then custom behaviors are registered.
pub struct MobEffectBehaviorRegistry {
    behaviors: Vec<Box<dyn MobEffectBehavior>>,
}

impl MobEffectBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all mob effects.
    #[must_use]
    pub fn new() -> Self {
        let count = REGISTRY.mob_effects.len();
        let mut behaviors: Vec<Box<dyn MobEffectBehavior>> = Vec::with_capacity(count);

        for _ in 0..count {
            behaviors.push(Box::new(DefaultMobEffect));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a mob effect.
    ///
    /// # Panics
    /// Panics if `effect` is not registered in the global registry.
    pub fn set_behavior(&mut self, effect: MobEffectRef, behavior: Box<dyn MobEffectBehavior>) {
        self.behaviors[effect.id()] = behavior;
    }

    /// Gets the behavior for a mob effect.
    ///
    /// # Panics
    /// Panics if `effect` is not registered in the global registry.
    #[must_use]
    pub fn get_behavior(&self, effect: MobEffectRef) -> &dyn MobEffectBehavior {
        self.behaviors[effect.id()].as_ref()
    }
}

impl Default for MobEffectBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
