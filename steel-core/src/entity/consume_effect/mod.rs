//! Consume-effect behaviors: one small module per vanilla `ConsumeEffect`
//! subtype under `net/minecraft/world/item/consume_effects`.

mod apply_effects;
mod clear_all_effects;
mod play_sound;
mod remove_effects;
mod teleport_randomly;

use std::sync::Arc;

use steel_registry::consume_effect::ConsumeEffectData;

pub use apply_effects::ApplyEffectsBehavior;
pub use clear_all_effects::ClearAllEffectsBehavior;
pub use play_sound::PlaySoundBehavior;
pub use remove_effects::RemoveEffectsBehavior;
pub use teleport_randomly::TeleportRandomlyBehavior;

use crate::entity::LivingEntity;
use crate::world::World;

/// Mirrors vanilla's `ConsumeEffect.apply(Level, ItemStack, LivingEntity)`
pub trait ConsumeEffectBehavior: Send + Sync {
    /// Applies this effect's behavior to `user`, downcasting `effect` to the
    /// concrete `ConsumeEffectData` payload this behavior expects.
    fn apply(&self, effect: &ConsumeEffectData, world: &Arc<World>, user: &dyn LivingEntity);
}
