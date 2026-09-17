//! `ApplyStatusEffectsConsumeEffect` behavior (golden apple, poisonous potato,
//! rotten flesh, etc.). Suspicious stew's effect is a separate, unrelated
//! `SuspiciousStewEffects` data component, not this.

use std::sync::Arc;

use steel_registry::consume_effect::{ApplyStatusEffectsConsumeEffect, ConsumeEffectData};

use super::ConsumeEffectBehavior;
use crate::entity::LivingEntity;
use crate::entity::potion_contents::to_runtime_instance;
use crate::world::World;

/// Mirrors vanilla `ApplyStatusEffectsConsumeEffect.apply`: unlike drinking a
/// potion, this always goes through `LivingEntity.addEffect`
pub struct ApplyEffectsBehavior;

impl ConsumeEffectBehavior for ApplyEffectsBehavior {
    fn apply(&self, effect: &ConsumeEffectData, _world: &Arc<World>, user: &dyn LivingEntity) {
        let Some(apply) = effect.downcast_ref::<ApplyStatusEffectsConsumeEffect>() else {
            return;
        };
        if rand::random::<f32>() >= apply.probability() {
            return;
        }
        for instance in apply.effects() {
            user.add_mob_effect(to_runtime_instance(instance, instance.duration()));
        }
    }
}
