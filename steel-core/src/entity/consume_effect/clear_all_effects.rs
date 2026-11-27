//! `ClearAllStatusEffectsConsumeEffect` behavior (e.g. milk bucket).

use std::sync::Arc;

use steel_registry::consume_effect::ConsumeEffectData;

use super::ConsumeEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `ClearAllStatusEffectsConsumeEffect.apply`. Carries no
/// fields, so there is nothing to downcast for.
pub struct ClearAllEffectsBehavior;

impl ConsumeEffectBehavior for ClearAllEffectsBehavior {
    fn apply(&self, _effect: &ConsumeEffectData, _world: &Arc<World>, user: &dyn LivingEntity) {
        for active in user.active_mob_effects() {
            user.remove_mob_effect(active.effect());
        }
    }
}
