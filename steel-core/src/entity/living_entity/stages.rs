//! `LivingEntity`'s layer of the pipelined vanilla methods.

use crate::entity::component::definition::{
    BaseExperienceRewardCtx, BaseTick, BaseTickCtx, BlockSpeedFactor, BlockSpeedFactorCtx,
    CanDispenserEquipIntoSlotCtx, CanFreeze, CanFreezeCtx, CanUseSlotCtx, CauseFallDamageCtx,
    CheckFallDamage, CheckFallDamageCtx, Definition, EquipSoundCtx, HurtCtx, SetYBodyRotCtx,
    TickCtx, VoicePitchCtx,
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
/// The `LivingEntity.rideTick` stage, after `Entity`'s.
pub const RIDE_TICK: StageId = StageId("living_entity.ride_tick");
/// The `LivingEntity.checkFallDamage` stage, around `Entity`'s.
pub const CHECK_FALL_DAMAGE: StageId = StageId("living_entity.check_fall_damage");
/// The `LivingEntity.causeFallDamage` stage, which replaces `Entity`'s.
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
/// The `LivingEntity.baseTick` stage, around `Entity`'s.
pub const BASE_TICK: StageId = StageId("living_entity.base_tick");

/// Vanilla `LivingEntity`'s chains, on `Entity`'s.
#[must_use]
pub fn definition<E: Parts>() -> Definition<E> {
    let definition = Definition::new();
    Definition {
        tick: definition.tick.then([Stage::new(TICK, tick::<E>)]),
        base_tick: definition.base_tick.around(BASE_TICK, base_tick::<E>),
        set_y_body_rot: definition
            .set_y_body_rot
            .then([Stage::new(SET_Y_BODY_ROT, set_y_body_rot::<E>)]),
        ride_tick: definition
            .ride_tick
            .then([Stage::effect(RIDE_TICK, |living: &E| {
                living.reset_fall_distance();
            })]),
        check_fall_damage: definition
            .check_fall_damage
            .around(CHECK_FALL_DAMAGE, check_fall_damage::<E>),
        cause_fall_damage: Pipeline::new([Stage::new(CAUSE_FALL_DAMAGE, cause_fall_damage::<E>)]),
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
        ..definition
    }
}

fn tick<E: Parts>(living: &E, ctx: &mut TickCtx<'_>) -> Flow {
    living.living().tick_living(ctx.entity);
    Flow::Continue
}

fn base_tick<E: Parts>(
    living: &E,
    ctx: &mut BaseTickCtx<'_>,
    inner: Inner<'_, E, BaseTick>,
) -> Flow {
    living.living().base_tick_around(|| inner(living, ctx))
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

fn cause_fall_damage<E: Parts>(living: &E, ctx: &mut CauseFallDamageCtx<'_>) -> Flow {
    ctx.result = living.living().cause_living_fall_damage(
        ctx.fall_distance,
        ctx.damage_modifier,
        ctx.source,
    );
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
