//! The player's `Definition`: vanilla `LivingEntity`'s chains with the
//! player's own stages.

use crate::entity::component::definition as definition_layer;
use crate::entity::component::definition::{
    AiStep, AiStepCtx, Definition, RideTick, RideTickCtx, TickCtx, Travel, TravelCtx, VoicePitchCtx,
};
use crate::entity::component::pipeline::{Flow, Inner, Pipeline, Stage, StageId};
use crate::entity::living_entity;

use super::Player;

const TICK: StageId = StageId("player.tick");
const HURT: StageId = StageId("player.hurt");
const VOICE_PITCH: StageId = StageId("player.voice_pitch");
const MACE_BURST: StageId = StageId("player.mace_burst");
const FLIGHT_FALL: StageId = StageId("player.flight_fall");
const FLIGHT_SPEED: StageId = StageId("player.flight_speed");
const SPECTATOR_FREEZE: StageId = StageId("player.spectator_freeze");
const SPECTATOR_SPLASH: StageId = StageId("player.spectator_splash");
const FLIGHT_SWIM: StageId = StageId("player.flight_swim");
const FLIGHT_STUCK: StageId = StageId("player.flight_stuck");
const RIDE_TICK: StageId = StageId("player.ride_tick");
const INVULNERABLE_ENEMY: StageId = StageId("player.invulnerable_enemy");
const DAMAGE_RULES: StageId = StageId("player.damage_rules");
const FLIGHT_GLIDE: StageId = StageId("player.flight_glide");
const SLEEPING_IMMOBILE: StageId = StageId("player.sleeping_immobile");
const JUMP_STATS: StageId = StageId("player.jump_stats");
const AI_STEP: StageId = StageId("player.ai_step");
const TRAVEL: StageId = StageId("player.travel");

impl Player {
    /// Vanilla `LivingEntity`'s chains, except that `ServerPlayer.tick` never
    /// calls `super`: the player runs the living tick's pieces itself, around
    /// the client-driven movement.
    pub(super) fn define() -> Definition<Self> {
        let living = living_entity::definition();
        Definition {
            tick: Pipeline::new([Stage::new(TICK, tick)]),
            // `Player.getVoicePitch` never calls `super`.
            voice_pitch: Pipeline::new([Stage::new(VOICE_PITCH, voice_pitch)]),
            check_fall_damage: living.check_fall_damage.before(
                living_entity::CHECK_FALL_DAMAGE,
                [Stage::new(MACE_BURST, Self::check_fall_damage_before_super)],
            ),
            cause_fall_damage: living.cause_fall_damage.before(
                living_entity::CAUSE_FALL_DAMAGE,
                [Stage::new(
                    FLIGHT_FALL,
                    Self::cause_fall_damage_before_super,
                )],
            ),
            block_speed_factor: living.block_speed_factor.before(
                living_entity::BLOCK_SPEED_FACTOR,
                [Stage::new(
                    FLIGHT_SPEED,
                    Self::block_speed_factor_before_super,
                )],
            ),
            can_freeze: living.can_freeze.before(
                living_entity::CAN_FREEZE,
                [Stage::new(SPECTATOR_FREEZE, Self::can_freeze_before_super)],
            ),
            do_water_splash_effect: living.do_water_splash_effect.before(
                definition_layer::DO_WATER_SPLASH_EFFECT,
                [Stage::new(
                    SPECTATOR_SPLASH,
                    Self::do_water_splash_effect_before_super,
                )],
            ),
            update_swimming: living.update_swimming.before(
                definition_layer::UPDATE_SWIMMING,
                [Stage::new(FLIGHT_SWIM, Self::update_swimming_before_super)],
            ),
            make_stuck_in_block: living.make_stuck_in_block.before(
                definition_layer::MAKE_STUCK_IN_BLOCK,
                [Stage::new(
                    FLIGHT_STUCK,
                    Self::make_stuck_in_block_before_super,
                )],
            ),
            ride_tick: living.ride_tick.around(RIDE_TICK, ride_tick),
            can_be_seen_as_enemy: living.can_be_seen_as_enemy.before(
                living_entity::CAN_BE_SEEN_AS_ENEMY,
                [Stage::new(
                    INVULNERABLE_ENEMY,
                    Self::can_be_seen_as_enemy_before_super,
                )],
            ),
            is_invulnerable_to: living.is_invulnerable_to.then([Stage::new(
                DAMAGE_RULES,
                Self::is_invulnerable_to_after_super,
            )]),
            can_glide: living.can_glide.before(
                living_entity::CAN_GLIDE,
                [Stage::new(FLIGHT_GLIDE, Self::can_glide_before_super)],
            ),
            is_immobile: living
                .is_immobile
                .then([Stage::new(SLEEPING_IMMOBILE, Self::is_immobile_after_super)]),
            jump_from_ground: living.jump_from_ground.then([Stage::effect(
                JUMP_STATS,
                Self::jump_from_ground_after_super,
            )]),
            ai_step: living.ai_step.around(AI_STEP, ai_step),
            travel: living.travel.around(TRAVEL, travel),
            hurt: living.hurt.before(
                living_entity::HURT,
                [Stage::new(HURT, Self::hurt_before_super)],
            ),
            ..living
        }
    }
}

fn tick(player: &Player, ctx: &mut TickCtx<'_>) -> Flow {
    player.tick_player(ctx.entity);
    Flow::Continue
}

/// `Player.getVoicePitch` never calls `super`.
const fn voice_pitch(_: &Player, ctx: &mut VoicePitchCtx<'_>) -> Flow {
    ctx.result = 1.0;
    Flow::Continue
}

fn ride_tick(
    player: &Player,
    ctx: &mut RideTickCtx<'_>,
    inner: Inner<'_, Player, RideTick>,
) -> Flow {
    player.ride_tick_around(|| {
        inner(player, ctx);
    });
    Flow::Continue
}

fn ai_step(player: &Player, ctx: &mut AiStepCtx<'_>, inner: Inner<'_, Player, AiStep>) -> Flow {
    let result = player.ai_step_around(|| {
        inner(player, ctx);
        ctx.result.take()
    });
    ctx.result = result;
    Flow::Continue
}

fn travel(player: &Player, ctx: &mut TravelCtx<'_>, inner: Inner<'_, Player, Travel>) -> Flow {
    let result = player.travel_around(|| {
        inner(player, ctx);
        ctx.result.take()
    });
    ctx.result = result;
    Flow::Continue
}
