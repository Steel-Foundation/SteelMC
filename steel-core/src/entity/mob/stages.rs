//! `Mob`'s own layers of the pipelined vanilla methods.

use crate::entity::component::definition::MobDefinition;
use crate::entity::component::entries::{AiStep, AiStepCtx, FinalizeSpawn, FinalizeSpawnCtx};
use crate::entity::component::pipeline::Pipeline;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::mob::Parts;

/// Stage running `LivingEntity.aiStep` and `Mob.aiStep`.
pub const AI_STEP: StageId = StageId("mob.ai_step");
/// Stage running `Mob.finalizeSpawn`.
pub const FINALIZE_SPAWN: StageId = StageId("mob.finalize_spawn");

/// The `Mob.aiStep` stage, including the `LivingEntity` base it calls first.
pub fn ai_step<E: Parts>() -> Stage<E, AiStep> {
    fn run<E: Parts>(entity: &E, ctx: &mut AiStepCtx<'_>) -> Flow {
        ctx.movement = entity.mob().mob_ai_step(ctx.entity);
        Flow::Continue
    }
    Stage::new(AI_STEP, run::<E>)
}

/// The `Mob.finalizeSpawn` stage: follow-range bonus and handedness.
pub fn finalize_spawn<E: Parts>() -> Stage<E, FinalizeSpawn> {
    fn run<E: Parts>(entity: &E, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        ctx.group_data =
            entity
                .mob()
                .finalize_spawn_mob_base(ctx.world, ctx.reason, ctx.group_data.take());
        Flow::Continue
    }
    Stage::new(FINALIZE_SPAWN, run::<E>)
}

/// Vanilla `Mob`'s own chains, for mobs without further layers.
#[must_use]
pub fn definition<E: Parts>() -> MobDefinition<E> {
    MobDefinition {
        construct: Pipeline::new([]),
        ai_step: Pipeline::new([ai_step::<E>()]),
        custom_server_ai_step: Pipeline::new([]),
        mob_interact: Pipeline::new([]),
        finalize_spawn: Pipeline::new([finalize_spawn::<E>()]),
    }
}
