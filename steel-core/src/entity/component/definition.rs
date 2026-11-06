//! Per-type definitions: the pipelines one entity type runs, built once.

use crate::entity::component::entries::{AiStep, CustomServerAiStep, FinalizeSpawn, MobInteract};
use crate::entity::component::pipeline::Pipeline;

/// Pipelines of one mob type, built from its layer presets.
pub struct MobDefinition<E: 'static> {
    /// `LivingEntity.aiStep`.
    pub ai_step: Pipeline<E, AiStep>,
    /// `Mob.customServerAiStep`.
    pub custom_server_ai_step: Pipeline<E, CustomServerAiStep>,
    /// `Mob.mobInteract`.
    pub mob_interact: Pipeline<E, MobInteract>,
    /// `Mob.finalizeSpawn`.
    pub finalize_spawn: Pipeline<E, FinalizeSpawn>,
}
