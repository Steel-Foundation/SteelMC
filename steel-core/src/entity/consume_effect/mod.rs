//! Consume-effect behaviors: one small module per vanilla `ConsumeEffect`
//! subtype under `net/minecraft/world/item/consume_effects`.
//!
//! Vanilla never branches on the concrete type: `Consumable.onConsume` just
//! calls `effect.apply(level, stack, user)` polymorphically, because each
//! subtype's data and behavior live on the same Java object. Steel can't do
//! that directly — the data (`ConsumeEffectData`) lives in `steel-registry`,
//! which can't depend on `LivingEntity`/`World` — so behavior is looked up by
//! `ConsumeEffectTypeRef` through [`CONSUME_EFFECT_BEHAVIORS`](crate::behavior::CONSUME_EFFECT_BEHAVIORS),
//! the same registry-of-trait-objects pattern `FLUID_BEHAVIORS`/
//! `BLOCK_BEHAVIORS` use for fluids and blocks.

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

/// One vanilla `ConsumeEffect` subtype's runtime behavior. Mirrors vanilla's
/// `ConsumeEffect.apply(Level, ItemStack, LivingEntity)`; Steel's `stack`
/// argument is dropped since no current implementation reads it (matching
/// vanilla, where none of the five subtypes use it either).
pub trait ConsumeEffectBehavior: Send + Sync {
    /// Applies this effect's behavior to `user`, downcasting `effect` to the
    /// concrete `ConsumeEffectData` payload this behavior expects.
    fn apply(&self, effect: &ConsumeEffectData, world: &Arc<World>, user: &dyn LivingEntity);
}
