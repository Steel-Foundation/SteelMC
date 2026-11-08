//! `Mob`'s own layers of the pipelined vanilla methods.

use crate::entity::component::definition::Definition;
use crate::entity::component::definition::{
    AiStep, AiStepCtx, Ate, FinalizeSpawn, FinalizeSpawnCtx, Tick,
};
use crate::entity::component::definition::{ControllingPassengerCtx, Interact, InteractCtx};
use crate::entity::component::pipeline::Inner;
use crate::entity::component::pipeline::Pipeline;
use crate::entity::component::pipeline::{Flow, Stage, StageId};
use crate::entity::living_entity;
use crate::entity::mob::Parts;
use steel_registry::vanilla_game_events;

/// Stage running `LivingEntity.aiStep` and `Mob.aiStep`.
pub const AI_STEP: StageId = StageId("mob.ai_step");
/// Stage running `Mob.finalizeSpawn`.
pub const FINALIZE_SPAWN: StageId = StageId("mob.finalize_spawn");
/// Stage running `Mob.baseTick` after the living base tick.
pub const BASE_TICK: StageId = StageId("mob.base_tick");
/// Stage running `Mob.interact` around `Entity`'s.
pub const INTERACT: StageId = StageId("mob.interact");
/// Stage running `Mob.getControllingPassenger`, which replaces `Entity`'s.
pub const CONTROLLING_PASSENGER: StageId = StageId("mob.controlling_passenger");
/// Stage running `Mob.tick` after the living tick.
pub const TICK: StageId = StageId("mob.tick");
/// Stage running `Mob.ate`.
pub const ATE: StageId = StageId("mob.ate");

/// The `Mob.aiStep` stage, including the `LivingEntity` base it calls first.
pub fn ai_step<E: Parts>() -> Stage<E, AiStep> {
    fn run<E: Parts>(entity: &E, ctx: &mut AiStepCtx<'_>) -> Flow {
        ctx.result = entity.mob().mob_ai_step(ctx.entity);
        Flow::Continue
    }
    Stage::new(AI_STEP, run::<E>)
}

/// The `Mob.ate` stage: the `EAT` game event.
pub fn ate<E: Parts>() -> Stage<E, Ate> {
    fn run<E: Parts>(entity: &E) {
        entity.game_event(&vanilla_game_events::EAT);
    }
    Stage::effect(ATE, run::<E>)
}

/// The `Mob.finalizeSpawn` stage: follow-range bonus and handedness.
pub fn finalize_spawn<E: Parts>() -> Stage<E, FinalizeSpawn> {
    fn run<E: Parts>(entity: &E, ctx: &mut FinalizeSpawnCtx<'_>) -> Flow {
        ctx.result = entity
            .mob()
            .finalize_spawn_mob_base(ctx.world, ctx.reason, ctx.result.take());
        Flow::Continue
    }
    Stage::new(FINALIZE_SPAWN, run::<E>)
}

/// The `Mob.tick` stage: the goal control flags every 5 ticks, whether or
/// not the AI runs.
pub fn tick<E: Parts>() -> Stage<E, Tick> {
    fn run<E: Parts>(entity: &E) {
        if entity.tick_count() % 5 == 0 {
            entity.mob().update_control_flags();
        }
    }
    Stage::effect(TICK, run::<E>)
}

/// Vanilla `Mob`'s own chains, for mobs without further layers.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    fn interact<E: Parts>(
        mob: &E,
        ctx: &mut InteractCtx<'_>,
        inner: Inner<'_, E, Interact>,
    ) -> Flow {
        let (player, hand) = (ctx.player, ctx.hand);
        let result = mob.mob().interact_around(player, hand, || {
            inner(mob, ctx);
            ctx.result
        });
        ctx.result = result;
        Flow::Continue
    }
    let living = living_entity::definition::<E>();
    Definition {
        interact: living.interact.around(INTERACT, interact::<E>),
        controlling_passenger: Pipeline::new([Stage::new(
            CONTROLLING_PASSENGER,
            |mob: &E, ctx: &mut ControllingPassengerCtx<'_>| {
                ctx.result = mob.mob().controlling_passenger_mob();
                Flow::Continue
            },
        )]),
        tick: living.tick.then([tick::<E>()]),
        base_tick: living.base_tick.then([Stage::effect(BASE_TICK, |mob: &E| {
            mob.mob().mob_base_tick();
        })]),
        ai_step: Pipeline::new([ai_step::<E>()]),
        ate: Pipeline::new([ate::<E>()]),
        finalize_spawn: Pipeline::new([finalize_spawn::<E>()]),
        ..living
    }
}
