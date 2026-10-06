//! `PlaySoundConsumeEffect` behavior (e.g. ominous bottle, via `Consumable
//! .Builder.soundAfterConsume`, sugar for this exact effect).

use std::sync::Arc;

use glam::DVec3;
use steel_registry::consume_effect::{ConsumeEffectData, PlaySoundConsumeEffect};

use super::ConsumeEffectBehavior;
use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla `PlaySoundConsumeEffect.apply`.
pub struct PlaySoundBehavior;

impl ConsumeEffectBehavior for PlaySoundBehavior {
    fn apply(&self, effect: &ConsumeEffectData, world: &Arc<World>, user: &dyn LivingEntity) {
        let Some(play_sound) = effect.downcast_ref::<PlaySoundConsumeEffect>() else {
            return;
        };
        let Some(sound) = play_sound.sound().registry_ref() else {
            return;
        };
        // Vanilla plays this at the entity's block-position center via
        // `Level.playSound(null, BlockPos, ...)`
        let block_pos = user.block_position();
        let block_center = DVec3::new(
            f64::from(block_pos.x()) + 0.5,
            f64::from(block_pos.y()) + 0.5,
            f64::from(block_pos.z()) + 0.5,
        );
        world.play_sound_at(sound, user.sound_source(), block_center, 1.0, 1.0, None);
    }
}
