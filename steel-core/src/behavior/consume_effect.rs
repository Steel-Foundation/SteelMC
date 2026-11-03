//! Consume-effect behavior registry.

use std::ops::Deref;
use std::sync::{Arc, OnceLock};

use steel_registry::consume_effect::{ConsumeEffectData, ConsumeEffectTypeRef};
use steel_registry::{REGISTRY, RegistryEntry, RegistryExt};

use crate::entity::LivingEntity;
use crate::entity::consume_effect::ConsumeEffectBehavior;
use crate::world::World;

/// No-op behavior for a consume-effect type without a registered behavior
/// (e.g. a mod-added type Steel doesn't implement yet).
struct NoopConsumeEffect;

impl ConsumeEffectBehavior for NoopConsumeEffect {
    fn apply(&self, _effect: &ConsumeEffectData, _world: &Arc<World>, _user: &dyn LivingEntity) {}
}

/// Wrapper for the global consume-effect behavior registry that implements `Deref`.
pub struct ConsumeEffectBehaviorLock(pub OnceLock<ConsumeEffectBehaviorRegistry>);

impl Deref for ConsumeEffectBehaviorLock {
    type Target = ConsumeEffectBehaviorRegistry;

    fn deref(&self) -> &Self::Target {
        self.0
            .get()
            .expect("Consume effect behaviors not initialized")
    }
}

/// Global consume-effect behavior registry.
///
/// Access behaviors directly via deref:
/// `CONSUME_EFFECT_BEHAVIORS.get_behavior(effect_type)`
pub static CONSUME_EFFECT_BEHAVIORS: ConsumeEffectBehaviorLock =
    ConsumeEffectBehaviorLock(OnceLock::new());

/// Registry for consume-effect behaviors.
///
/// Created after the main registry is frozen. All consume-effect types are
/// initialized with a default no-op behavior, then custom behaviors are
/// registered.
pub struct ConsumeEffectBehaviorRegistry {
    behaviors: Vec<Box<dyn ConsumeEffectBehavior>>,
}

impl ConsumeEffectBehaviorRegistry {
    /// Creates a new behavior registry with default behaviors for all consume-effect types.
    #[must_use]
    pub fn new() -> Self {
        let count = REGISTRY.consume_effect_types.len();
        let mut behaviors: Vec<Box<dyn ConsumeEffectBehavior>> = Vec::with_capacity(count);

        for _ in 0..count {
            behaviors.push(Box::new(NoopConsumeEffect));
        }

        Self { behaviors }
    }

    /// Sets a custom behavior for a consume-effect type.
    ///
    /// # Panics
    /// Panics if `effect_type` is not registered in the global registry.
    pub fn set_behavior(
        &mut self,
        effect_type: ConsumeEffectTypeRef,
        behavior: Box<dyn ConsumeEffectBehavior>,
    ) {
        self.behaviors[effect_type.id()] = behavior;
    }

    /// Gets the behavior for a consume-effect type.
    ///
    /// # Panics
    /// Panics if `effect_type` is not registered in the global registry.
    #[must_use]
    pub fn get_behavior(&self, effect_type: ConsumeEffectTypeRef) -> &dyn ConsumeEffectBehavior {
        self.behaviors[effect_type.id()].as_ref()
    }
}

impl Default for ConsumeEffectBehaviorRegistry {
    fn default() -> Self {
        Self::new()
    }
}
