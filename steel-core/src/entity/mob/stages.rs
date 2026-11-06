//! `Mob`'s own layers of the pipelined vanilla methods.

use crate::entity::Mob;
use crate::entity::component::entries::{AiStep, AiStepCtx, FinalizeSpawn, FinalizeSpawnCtx};
use crate::entity::component::pipeline::{Flow, Stage, StageId};

/// Stage running `LivingEntity.aiStep` and `Mob.aiStep`.
pub const AI_STEP: StageId = StageId("mob.ai_step");
/// Stage running `Mob.finalizeSpawn`.
pub const FINALIZE_SPAWN: StageId = StageId("mob.finalize_spawn");

/// The `Mob.aiStep` stage, including the `LivingEntity` base it calls first.
pub fn ai_step<E: Mob>() -> Stage<E, AiStep> {
    fn run<E: Mob>(entity: &E, ctx: &mut AiStepCtx<'_>) -> Flow {
        ctx.movement = entity.mob_ai_step(ctx.entity);
        Flow::Continue
    }
    Stage::new(AI_STEP, run::<E>)
}

/// The `Mob.finalizeSpawn` stage: follow-range bonus and handedness.
pub fn finalize_spawn<E: Mob>() -> Stage<E, FinalizeSpawn> {
    fn run<E: Mob>(entity: &E, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        ctx.group_data =
            entity.finalize_spawn_mob_base(ctx.world, ctx.reason, ctx.group_data.take());
        Flow::Continue
    }
    Stage::new(FINALIZE_SPAWN, run::<E>)
}
