//! `RemoveStatusEffectsConsumeEffect` behavior (e.g. honey bottle removing Poison).

use std::sync::Arc;

use steel_registry::consume_effect::{ConsumeEffectData, RemoveStatusEffectsConsumeEffect};

use super::ConsumeEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `RemoveStatusEffectsConsumeEffect.apply`.
pub struct RemoveEffectsBehavior;

impl ConsumeEffectBehavior for RemoveEffectsBehavior {
    fn apply(&self, effect: &ConsumeEffectData, _world: &Arc<World>, user: &dyn LivingEntity) {
        let Some(remove) = effect.downcast_ref::<RemoveStatusEffectsConsumeEffect>() else {
            return;
        };
        for active in user.active_mob_effects() {
            if remove.effects().contains(active.effect()) {
                user.remove_mob_effect(active.effect());
            }
        }
    }
}
