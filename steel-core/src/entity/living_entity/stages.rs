//! `LivingEntity`'s layer of the pipelined vanilla methods.

use crate::entity::component::definition::{
    AiStepCtx, CanBeSeenAsEnemyCtx, CanGlideCtx, IsImmobileCtx, IsInvulnerableToCtx, TravelCtx,
};
use crate::entity::component::definition::{
    BaseExperienceRewardCtx, BaseTick, BaseTickCtx, BlockSpeedFactor, BlockSpeedFactorCtx,
    CanDispenserEquipIntoSlotCtx, CanFreeze, CanFreezeCtx, CanUseSlotCtx, CauseFallDamage,
    CauseFallDamageCtx, CheckFallDamage, CheckFallDamageCtx, Definition, EquipSoundCtx, HurtCtx,
    SetYBodyRotCtx, StopRiding, StopRidingCtx, VoicePitchCtx,
};
use crate::entity::component::pipeline::StageId;
use crate::entity::component::pipeline::{Flow, Inner, Pipeline, Stage};
use crate::entity::prelude::*;

use super::Parts;

/// The `LivingEntity.tick` stage, after `Entity`'s.
pub const TICK: StageId = StageId("living_entity.tick");
/// The `LivingEntity.hurtServer` stage; `Entity`'s is abstract.
pub const HURT: StageId = StageId("living_entity.hurt");
/// The `LivingEntity.setYBodyRot` stage: stores the body yaw.
pub const SET_Y_BODY_ROT: StageId = StageId("living_entity.set_y_body_rot");
/// The `LivingEntity.stopRiding` stage: places the entity beside its old
/// vehicle, around `Entity`'s.
pub const STOP_RIDING: StageId = StageId("living_entity.stop_riding");
/// The `LivingEntity.rideTick` stage, after `Entity`'s.
pub const RIDE_TICK: StageId = StageId("living_entity.ride_tick");
/// The `LivingEntity.checkFallDamage` stage, around `Entity`'s.
pub const CHECK_FALL_DAMAGE: StageId = StageId("living_entity.check_fall_damage");
/// The `LivingEntity.causeFallDamage` stage, around `Entity`'s.
pub const CAUSE_FALL_DAMAGE: StageId = StageId("living_entity.cause_fall_damage");
/// The `LivingEntity.canFreeze` stage, around `Entity`'s.
pub const CAN_FREEZE: StageId = StageId("living_entity.can_freeze");
/// The `LivingEntity.getBlockSpeedFactor` stage, around `Entity`'s.
pub const BLOCK_SPEED_FACTOR: StageId = StageId("living_entity.block_speed_factor");
/// The `LivingEntity.getVoicePitch` stage.
pub const VOICE_PITCH: StageId = StageId("living_entity.voice_pitch");
/// The `LivingEntity.getEquipSound` stage.
pub const EQUIP_SOUND: StageId = StageId("living_entity.equip_sound");
/// The `LivingEntity.getBaseExperienceReward` stage: none.
pub const BASE_EXPERIENCE_REWARD: StageId = StageId("living_entity.base_experience_reward");
/// The `LivingEntity.canUseSlot` stage: any slot.
pub const CAN_USE_SLOT: StageId = StageId("living_entity.can_use_slot");
/// The `LivingEntity.canDispenserEquipIntoSlot` stage: any slot.
pub const CAN_DISPENSER_EQUIP_INTO_SLOT: StageId =
    StageId("living_entity.can_dispenser_equip_into_slot");
/// The `LivingEntity.aiStep` stage.
pub const AI_STEP: StageId = StageId("living_entity.ai_step");
/// The `LivingEntity.travel` stage.
pub const TRAVEL: StageId = StageId("living_entity.travel");
/// The `LivingEntity.jumpFromGround` stage.
pub const JUMP_FROM_GROUND: StageId = StageId("living_entity.jump_from_ground");
/// The `LivingEntity.isImmobile` stage.
pub const IS_IMMOBILE: StageId = StageId("living_entity.is_immobile");
/// The `LivingEntity.canGlide` stage.
pub const CAN_GLIDE: StageId = StageId("living_entity.can_glide");
/// The `LivingEntity.isInvulnerableTo` stage.
pub const IS_INVULNERABLE_TO: StageId = StageId("living_entity.is_invulnerable_to");
/// The `LivingEntity.canBeSeenAsEnemy` stage.
pub const CAN_BE_SEEN_AS_ENEMY: StageId = StageId("living_entity.can_be_seen_as_enemy");
/// The `LivingEntity.baseTick` stage, around `Entity`'s.
pub const BASE_TICK: StageId = StageId("living_entity.base_tick");

/// Vanilla `LivingEntity`'s chains, on `Entity`'s.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    let definition = Definition::new();
    Definition {
        tick: definition.tick.then([Stage::effect(TICK, tick::<E>)]),
        base_tick: definition.base_tick.around(BASE_TICK, base_tick::<E>),
        set_y_body_rot: definition
            .set_y_body_rot
            .then([Stage::new(SET_Y_BODY_ROT, set_y_body_rot::<E>)]),
        stop_riding: definition.stop_riding.around(STOP_RIDING, stop_riding::<E>),
        ride_tick: definition
            .ride_tick
            .then([Stage::effect(RIDE_TICK, |living: &E| {
                living.reset_fall_distance();
            })]),
        check_fall_damage: definition
            .check_fall_damage
            .around(CHECK_FALL_DAMAGE, check_fall_damage::<E>),
        cause_fall_damage: definition
            .cause_fall_damage
            .around(CAUSE_FALL_DAMAGE, cause_fall_damage::<E>),
        can_freeze: definition.can_freeze.around(CAN_FREEZE, can_freeze::<E>),
        block_speed_factor: definition
            .block_speed_factor
            .around(BLOCK_SPEED_FACTOR, block_speed_factor::<E>),
        voice_pitch: Pipeline::new([Stage::new(VOICE_PITCH, voice_pitch::<E>)]),
        equip_sound: Pipeline::new([Stage::new(EQUIP_SOUND, equip_sound::<E>)]),
        base_experience_reward: Pipeline::new([Stage::new(
            BASE_EXPERIENCE_REWARD,
            |_: &E, ctx: &mut BaseExperienceRewardCtx<'_>| {
                ctx.result = 0;
                Flow::Continue
            },
        )]),
        can_use_slot: Pipeline::new([Stage::new(
            CAN_USE_SLOT,
            |_: &E, ctx: &mut CanUseSlotCtx<'_>| {
                ctx.result = true;
                Flow::Continue
            },
        )]),
        can_dispenser_equip_into_slot: Pipeline::new([Stage::new(
            CAN_DISPENSER_EQUIP_INTO_SLOT,
            |_: &E, ctx: &mut CanDispenserEquipIntoSlotCtx<'_>| {
                ctx.result = true;
                Flow::Continue
            },
        )]),
        // Vanilla's `LivingEntity.tickRidden` does nothing.
        tick_ridden: Pipeline::new([]),
        hurt: Pipeline::new([Stage::new(HURT, hurt::<E>)]),
        ai_step: Pipeline::new([Stage::new(
            AI_STEP,
            |living: &E, ctx: &mut AiStepCtx<'_>| {
                ctx.result = living.living().default_ai_step();
                Flow::Continue
            },
        )]),
        travel: Pipeline::new([Stage::new(TRAVEL, |living: &E, ctx: &mut TravelCtx<'_>| {
            ctx.result = living.living().default_travel(ctx.input);
            Flow::Continue
        })]),
        jump_from_ground: Pipeline::new([Stage::effect(JUMP_FROM_GROUND, |living: &E| {
            living.living().default_jump_from_ground();
        })]),
        is_immobile: Pipeline::new([Stage::new(
            IS_IMMOBILE,
            |living: &E, ctx: &mut IsImmobileCtx<'_>| {
                ctx.result = living.living().default_is_immobile();
                Flow::Continue
            },
        )]),
        can_glide: Pipeline::new([Stage::new(
            CAN_GLIDE,
            |living: &E, ctx: &mut CanGlideCtx<'_>| {
                ctx.result = living.living().default_can_glide();
                Flow::Continue
            },
        )]),
        is_invulnerable_to: Pipeline::new([Stage::new(
            IS_INVULNERABLE_TO,
            |living: &E, ctx: &mut IsInvulnerableToCtx<'_>| {
                ctx.result = living.living().is_invulnerable_to_living(ctx.source);
                Flow::Continue
            },
        )]),
        can_be_seen_as_enemy: Pipeline::new([Stage::new(
            CAN_BE_SEEN_AS_ENEMY,
            |living: &E, ctx: &mut CanBeSeenAsEnemyCtx<'_>| {
                ctx.result = living.living().can_be_seen_as_enemy_base();
                Flow::Continue
            },
        )]),
        ..definition
    }
}

fn tick<E: Parts>(living: &E) {
    living.living().tick_living();
}

fn base_tick<E: Parts>(
    living: &E,
    ctx: &mut BaseTickCtx<'_>,
    inner: Inner<'_, E, BaseTick>,
) -> Flow {
    living.living().base_tick_around(|| inner(living, ctx))
}

fn stop_riding<E: Parts>(
    living: &E,
    ctx: &mut StopRidingCtx<'_>,
    inner: Inner<'_, E, StopRiding>,
) -> Flow {
    let old_vehicle = living.vehicle();
    let flow = inner(living, ctx);
    if let Some(old_vehicle) = old_vehicle
        && living.vehicle().map(|vehicle| vehicle.id()) != Some(old_vehicle.id())
    {
        living.living().dismount_vehicle(old_vehicle.as_ref());
    }
    flow
}

fn set_y_body_rot<E: Parts>(living: &E, ctx: &mut SetYBodyRotCtx<'_>) -> Flow {
    living.living().set_y_body_rot(ctx.y_body_rot);
    Flow::Continue
}

fn check_fall_damage<E: Parts>(
    living: &E,
    ctx: &mut CheckFallDamageCtx<'_>,
    inner: Inner<'_, E, CheckFallDamage>,
) -> Flow {
    let (on_ground, on_state, pos, world) = (ctx.on_ground, ctx.on_state, ctx.pos, ctx.world);
    living
        .living()
        .check_fall_damage_around(on_ground, on_state, pos, world, || {
            inner(living, ctx);
        });
    Flow::Continue
}

fn cause_fall_damage<E: Parts>(
    living: &E,
    ctx: &mut CauseFallDamageCtx<'_>,
    inner: Inner<'_, E, CauseFallDamage>,
) -> Flow {
    let (fall_distance, damage_modifier, source) =
        (ctx.fall_distance, ctx.damage_modifier, ctx.source);
    let result = living.living().cause_living_fall_damage(
        fall_distance,
        damage_modifier,
        source,
        |effective_fall_distance| {
            ctx.fall_distance = effective_fall_distance;
            inner(living, ctx);
            ctx.result
        },
    );
    ctx.result = result;
    Flow::Continue
}

fn can_freeze<E: Parts>(
    living: &E,
    ctx: &mut CanFreezeCtx<'_>,
    inner: Inner<'_, E, CanFreeze>,
) -> Flow {
    let result = living.living().can_freeze_around(|| {
        inner(living, ctx);
        ctx.result
    });
    ctx.result = result;
    Flow::Continue
}

fn block_speed_factor<E: Parts>(
    living: &E,
    ctx: &mut BlockSpeedFactorCtx<'_>,
    inner: Inner<'_, E, BlockSpeedFactor>,
) -> Flow {
    let result = living.living().block_speed_factor_around(|| {
        inner(living, ctx);
        ctx.result
    });
    ctx.result = result;
    Flow::Continue
}

fn voice_pitch<E: Parts>(living: &E, ctx: &mut VoicePitchCtx<'_>) -> Flow {
    ctx.result = living.living().voice_pitch_base();
    Flow::Continue
}

fn equip_sound<E: Parts>(living: &E, ctx: &mut EquipSoundCtx<'_>) -> Flow {
    ctx.result = living.living().equip_sound_base(ctx.slot, ctx.stack);
    Flow::Continue
}

fn hurt<E: Parts>(living: &E, ctx: &mut HurtCtx<'_>) -> Flow {
    ctx.result = living
        .living()
        .hurt_server(ctx.world, ctx.source, ctx.amount);
    Flow::Continue
}
