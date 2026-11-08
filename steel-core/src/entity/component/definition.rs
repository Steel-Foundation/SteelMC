//! Per-type definitions: the pipelines one entity type runs, built once.
//!
//! Every vanilla method that subclasses extend with `super` runs as a chain
//! here, so an entity type adds stages to its layers' chain instead of calling
//! the layer by hand. Methods overridden without `super` stay hooks in the
//! layer's props.
//!
//! [`definition_chains!`] declares each chain once and generates its
//! [`Entry`], the context its stages share, the [`Definition`] field and the
//! [`DefinitionEntries`] dispatch. A chain returning a value carries it in the
//! context's `result`, which starts at the declared default, or at a value the
//! caller passes when the chain declares none.
//!
//! Chains whose arguments borrow declare `<'a>`, the context's lifetime:
//! `fn on_hit<'a>(entity: &'a SharedEntity, hit: &'a ProjectileHit);`.

use std::marker::PhantomData;
use std::sync::{Arc, LazyLock};

use glam::DVec3;
use steel_utils::types::InteractionHand;
use steel_utils::{Downcast as _, DowncastType};

use crate::behavior::InteractionResult;
use crate::entity::component::pipeline::{Entry, Flow, Pipeline, Stage, StageId};
use crate::entity::damage::DamageSource;
use crate::entity::prelude::*;
use crate::entity::projectile::ProjectileHit;
use crate::entity::{
    Entity, EntitySpawnReason, MobBase, SharedEntity, SpawnGroupData, position_rider_default,
};
use crate::inventory::equipment::EquipmentSlot;
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::{ClipHitResult, World};
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_event::SoundEventRef;
use steel_utils::{BlockPos, BlockStateId};

/// Declares the chains of [`Definition`]; see the [module docs](self).
macro_rules! definition_chains {
    (
        $(
            $(#[$meta:meta])*
            $entry:ident / $ctx:ident:
                fn $name:ident $(<$lt:lifetime>)? ($($arg:ident: $ty:ty),* $(,)?)
                    $(-> $ret:ty $(= $init:expr)?)?;
        )*
    ) => {
        $(
            $(#[$meta])*
            pub struct $entry;

            #[doc = concat!("Context of one [`", stringify!($entry), "`] call.")]
            pub struct $ctx<'a> {
                $(
                    #[doc = concat!("The `", stringify!($arg), "` argument.")]
                    pub $arg: $ty,
                )*
                $(
                    /// The chain's result so far.
                    pub result: $ret,
                )?
                _marker: PhantomData<&'a ()>,
            }

            impl Entry for $entry {
                type Ctx<'a> = $ctx<'a>;
            }
        )*

        /// Pipelines of one entity type, built from its layer presets.
        ///
        /// A layer's pipelines stay empty for entities without that layer.
        pub struct Definition<E: ?Sized + 'static> {
            /// The mob constructor chain, `Mob.registerGoals` included.
            pub construct: Pipeline<MobBase, Construct>,
            $(
                $(#[$meta])*
                pub $name: Pipeline<E, $entry>,
            )*
        }

        impl<E: ?Sized + 'static> Definition<E> {
            /// Every chain empty.
            fn empty() -> Self {
                Self {
                    construct: Pipeline::new([]),
                    $( $name: Pipeline::new([]), )*
                }
            }
        }

        /// A [`Definition`] reached from code that only holds `&dyn Entity`.
        ///
        /// Implemented once for every definition, so entity types never forward
        /// entry points by hand.
        #[expect(
            clippy::too_many_arguments,
            reason = "chains take vanilla's arguments plus the entity"
        )]
        pub trait DefinitionEntries: Send + Sync {
            /// Runs the mob constructor chain on a fresh `mob_base`.
            fn construct(&self, mob_base: &MobBase);

            $(
                definition_chains!(
                    @trait $name ($($lt)?) ($($arg: $ty),*) ($($ret $(= $init)?)?)
                );
            )*
        }

        impl<E: DefinitionTarget + ?Sized> DefinitionEntries for Definition<E> {
            fn construct(&self, mob_base: &MobBase) {
                self.construct.run(mob_base, &mut ());
            }

            $(
                definition_chains!(
                    @impl $name $ctx ($($lt)?) ($($arg: $ty),*) ($($ret $(= $init)?)?)
                );
            )*
        }
    };
    (@trait $name:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ()) => {
        #[doc = concat!("Runs `", stringify!($name), "` for `entity`.")]
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*);
    };
    (@trait $name:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ($ret:ty = $init:expr)) => {
        #[doc = concat!("Runs `", stringify!($name), "` for `entity`.")]
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*) -> $ret;
    };
    (@trait $name:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ($ret:ty)) => {
        #[doc = concat!("Runs `", stringify!($name), "` for `entity`, starting from `initial`.")]
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*, initial: $ret) -> $ret;
    };
    (@impl $name:ident $ctx:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ()) => {
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*) {
            if let Some(typed) = E::typed(entity) {
                let mut ctx = $ctx {
                    $($arg,)*
                    _marker: PhantomData,
                };
                self.$name.run(typed, &mut ctx);
            }
        }
    };
    (@impl $name:ident $ctx:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ($ret:ty = $init:expr)) => {
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*) -> $ret {
            match E::typed(entity) {
                Some(typed) => {
                    let mut ctx = $ctx {
                        $($arg,)*
                        result: $init,
                        _marker: PhantomData,
                    };
                    self.$name.run(typed, &mut ctx);
                    ctx.result
                }
                None => $init,
            }
        }
    };
    (@impl $name:ident $ctx:ident ($($lt:lifetime)?) ($($arg:ident: $ty:ty),*) ($ret:ty)) => {
        fn $name $(<$lt>)? (&self, entity: &dyn Entity $(, $arg: $ty)*, initial: $ret) -> $ret {
            match E::typed(entity) {
                Some(typed) => {
                    let mut ctx = $ctx {
                        $($arg,)*
                        result: initial,
                        _marker: PhantomData,
                    };
                    self.$name.run(typed, &mut ctx);
                    ctx.result
                }
                None => initial,
            }
        }
    };
}

/// The mob constructor chain, `Mob.registerGoals` included. Runs on the fresh
/// `MobBase` before the entity exists.
pub struct Construct;

impl Entry for Construct {
    type Ctx<'a> = ();
}

definition_chains! {
    /// Vanilla `Entity.tick`.
    Tick / TickCtx: fn tick<'a>(entity: &'a SharedEntity);
    /// Vanilla `Entity.baseTick`.
    BaseTick / BaseTickCtx: fn base_tick();
    /// Vanilla `Entity.hurtServer`; stages may change `amount` for later ones.
    /// Vanilla `Entity`'s is abstract, so the chain starts empty.
    Hurt / HurtCtx:
        fn hurt<'a>(world: &'a World, source: &'a DamageSource, amount: f32) -> bool = false;
    /// Vanilla `Entity.rideTick`: a passenger's tick.
    RideTick / RideTickCtx: fn ride_tick<'a>(entity: &'a SharedEntity);
    /// Vanilla `Entity.checkFallDamage`: accumulates fall distance and lands.
    CheckFallDamage / CheckFallDamageCtx:
        fn check_fall_damage<'a>(
            entity: &'a SharedEntity,
            vertical_movement: f64,
            on_ground: bool,
            on_state: BlockStateId,
            pos: BlockPos,
            world: &'a Arc<World>,
        );
    /// Vanilla `Entity.causeFallDamage`.
    CauseFallDamage / CauseFallDamageCtx:
        fn cause_fall_damage<'a>(
            entity: &'a SharedEntity,
            fall_distance: f64,
            damage_modifier: f32,
            source: &'a DamageSource,
        ) -> bool = false;
    /// Vanilla `Entity.canFreeze`.
    CanFreeze / CanFreezeCtx: fn can_freeze() -> bool = false;
    /// Vanilla `Entity.makeStuckInBlock`.
    MakeStuckInBlock / MakeStuckInBlockCtx:
        fn make_stuck_in_block(state: BlockStateId, speed_multiplier: DVec3);
    /// Vanilla `Entity.doWaterSplashEffect`.
    DoWaterSplashEffect / DoWaterSplashEffectCtx: fn do_water_splash_effect();
    /// Vanilla `Entity.updateSwimming`.
    UpdateSwimming / UpdateSwimmingCtx: fn update_swimming();
    /// Vanilla `Entity.stopRiding`.
    StopRiding / StopRidingCtx: fn stop_riding();
    /// Vanilla `Entity.removeVehicle`: leaves the vehicle without placing
    /// the entity.
    RemoveVehicle / RemoveVehicleCtx: fn remove_vehicle();
    /// Vanilla `Entity.getBlockSpeedFactor`.
    BlockSpeedFactor / BlockSpeedFactorCtx: fn block_speed_factor() -> f32 = 1.0;
    /// Vanilla `Entity.interact`.
    Interact / InteractCtx:
        fn interact<'a>(player: &'a Player, hand: InteractionHand, location: DVec3)
            -> InteractionResult = InteractionResult::Pass;
    /// Vanilla `Entity.setYBodyRot`.
    SetYBodyRot / SetYBodyRotCtx: fn set_y_body_rot(y_body_rot: f32);
    /// Vanilla `Entity.positionRider`: places a direct passenger on this vehicle.
    PositionRider / PositionRiderCtx: fn position_rider<'a>(passenger: &'a dyn Entity);
    /// Vanilla `Entity.getControllingPassenger`.
    ControllingPassenger / ControllingPassengerCtx:
        fn controlling_passenger() -> Option<SharedEntity> = None;
    /// Vanilla `LivingEntity.travel`, with the movement it performed.
    Travel / TravelCtx:
        fn travel<'a>(entity: &'a SharedEntity, input: DVec3) -> Option<MoveResult> = None;
    /// Vanilla `LivingEntity.jumpFromGround`.
    JumpFromGround / JumpFromGroundCtx: fn jump_from_ground();
    /// Vanilla `LivingEntity.isImmobile`.
    IsImmobile / IsImmobileCtx: fn is_immobile() -> bool = false;
    /// Vanilla `LivingEntity.canGlide`.
    CanGlide / CanGlideCtx: fn can_glide() -> bool = false;
    /// Vanilla `LivingEntity.isInvulnerableTo`.
    IsInvulnerableTo / IsInvulnerableToCtx:
        fn is_invulnerable_to<'a>(world: &'a World, source: &'a DamageSource) -> bool = false;
    /// Vanilla `LivingEntity.canBeSeenAsEnemy`.
    CanBeSeenAsEnemy / CanBeSeenAsEnemyCtx: fn can_be_seen_as_enemy() -> bool = false;
    /// Vanilla `LivingEntity.getVoicePitch`.
    VoicePitch / VoicePitchCtx: fn voice_pitch() -> f32 = 1.0;
    /// Vanilla `LivingEntity.getEquipSound`, as far as Steel resolves it.
    EquipSound / EquipSoundCtx:
        fn equip_sound<'a>(slot: EquipmentSlot, stack: &'a ItemStack)
            -> Option<SoundEventRef> = None;
    /// Vanilla `LivingEntity.getBaseExperienceReward`.
    BaseExperienceReward / BaseExperienceRewardCtx: fn base_experience_reward() -> i32 = 0;
    /// Vanilla `LivingEntity.canUseSlot`.
    CanUseSlot / CanUseSlotCtx: fn can_use_slot(slot: EquipmentSlot) -> bool = true;
    /// Vanilla `LivingEntity.canDispenserEquipIntoSlot`.
    CanDispenserEquipIntoSlot / CanDispenserEquipIntoSlotCtx:
        fn can_dispenser_equip_into_slot(slot: EquipmentSlot) -> bool = true;
    /// Vanilla `LivingEntity.tickRidden`: side effects of a rider steering
    /// this entity.
    TickRidden / TickRiddenCtx: fn tick_ridden<'a>(controller: &'a Player, ridden_input: DVec3);
    /// Vanilla `Projectile.onHit`.
    OnHit / OnHitCtx: fn on_hit<'a>(entity: &'a SharedEntity, hit: &'a ProjectileHit);
    /// Vanilla `Projectile.onHitEntity`.
    OnHitEntity / OnHitEntityCtx:
        fn on_hit_entity<'a>(entity: &'a SharedEntity, target: &'a SharedEntity, location: DVec3);
    /// Vanilla `Projectile.onHitBlock`.
    OnHitBlock / OnHitBlockCtx: fn on_hit_block<'a>(entity: &'a SharedEntity, hit: &'a ClipHitResult);
    /// Vanilla `Projectile.canHitEntity`.
    CanHitEntity / CanHitEntityCtx: fn can_hit_entity<'a>(target: &'a dyn Entity) -> bool = false;
    /// Vanilla `LivingEntity.aiStep`, with the movement it performed.
    AiStep / AiStepCtx: fn ai_step<'a>(entity: &'a SharedEntity) -> Option<MoveResult> = None;
    /// Vanilla `Mob.customServerAiStep`.
    CustomServerAiStep / CustomServerAiStepCtx: fn custom_server_ai_step();
    /// Vanilla `Mob.ate`, run after an eating goal consumes a block.
    Ate / AteCtx: fn ate();
    /// Vanilla `Mob.mobInteract`: the first stage that handles the interaction
    /// sets the result and stops.
    MobInteract / MobInteractCtx:
        fn mob_interact<'a>(player: &'a Player, hand: InteractionHand)
            -> InteractionResult = InteractionResult::Pass;
    /// Vanilla `Mob.finalizeSpawn`; the result is the group data for the next
    /// group member, starting at the caller's.
    FinalizeSpawn / FinalizeSpawnCtx:
        fn finalize_spawn<'a>(world: &'a Arc<World>, reason: EntitySpawnReason)
            -> Option<SpawnGroupData>;
}

/// The `Entity.tick` stage: vanilla's base entity tick, `Entity.baseTick`.
pub const TICK: StageId = StageId("entity.tick");
/// The `Entity.baseTick` stage.
pub const BASE_TICK: StageId = StageId("entity.base_tick");
/// The `Entity.rideTick` stage.
pub const RIDE_TICK: StageId = StageId("entity.ride_tick");
/// The `Entity.checkFallDamage` stage.
pub const CHECK_FALL_DAMAGE: StageId = StageId("entity.check_fall_damage");
/// The `Entity.causeFallDamage` stage.
pub const CAUSE_FALL_DAMAGE: StageId = StageId("entity.cause_fall_damage");
/// The `Entity.canFreeze` stage.
pub const CAN_FREEZE: StageId = StageId("entity.can_freeze");
/// The `Entity.makeStuckInBlock` stage.
pub const MAKE_STUCK_IN_BLOCK: StageId = StageId("entity.make_stuck_in_block");
/// The `Entity.doWaterSplashEffect` stage.
pub const DO_WATER_SPLASH_EFFECT: StageId = StageId("entity.do_water_splash_effect");
/// The `Entity.updateSwimming` stage.
pub const UPDATE_SWIMMING: StageId = StageId("entity.update_swimming");
/// The `Entity.stopRiding` stage: removes the vehicle.
pub const STOP_RIDING: StageId = StageId("entity.stop_riding");
/// The `Entity.removeVehicle` stage.
pub const REMOVE_VEHICLE: StageId = StageId("entity.remove_vehicle");
/// The `Entity.getBlockSpeedFactor` stage.
pub const BLOCK_SPEED_FACTOR: StageId = StageId("entity.block_speed_factor");
/// The `Entity.interact` stage: leashing and unleashing.
pub const INTERACT: StageId = StageId("entity.interact");
/// The `Entity.positionRider` stage.
pub const POSITION_RIDER: StageId = StageId("entity.position_rider");
/// The `Entity.getControllingPassenger` stage: entities have none.
pub const CONTROLLING_PASSENGER: StageId = StageId("entity.controlling_passenger");

impl<E: Entity + ?Sized> Definition<E> {
    /// Vanilla `Entity`'s chains.
    #[must_use]
    pub fn new() -> Self {
        fn tick<E: Entity + ?Sized>(entity: &E) {
            entity.base_tick();
        }
        fn base_tick<E: Entity + ?Sized>(entity: &E) {
            entity.entity_base_tick();
        }
        fn interact<E: Entity + ?Sized>(entity: &E, ctx: &mut InteractCtx<'_>) -> Flow {
            ctx.result = entity.interact_default(ctx.player, ctx.hand, ctx.location);
            Flow::Continue
        }
        fn position_rider<E: Entity + ?Sized>(entity: &E, ctx: &mut PositionRiderCtx<'_>) -> Flow {
            position_rider_default(entity, ctx.passenger);
            Flow::Continue
        }
        fn ride_tick<E: Entity + ?Sized>(_: &E, ctx: &mut RideTickCtx<'_>) -> Flow {
            Arc::clone(ctx.entity).ride_tick_default();
            Flow::Continue
        }
        fn check_fall_damage<E: Entity + ?Sized>(_: &E, ctx: &mut CheckFallDamageCtx<'_>) -> Flow {
            Arc::clone(ctx.entity).check_fall_damage_default(
                ctx.vertical_movement,
                ctx.on_ground,
                ctx.on_state,
                ctx.pos,
                ctx.world,
            );
            Flow::Continue
        }
        fn cause_fall_damage<E: Entity + ?Sized>(_: &E, ctx: &mut CauseFallDamageCtx<'_>) -> Flow {
            ctx.result = Arc::clone(ctx.entity).cause_fall_damage_default(
                ctx.fall_distance,
                ctx.damage_modifier,
                ctx.source,
            );
            Flow::Continue
        }
        fn can_freeze<E: Entity + ?Sized>(entity: &E, ctx: &mut CanFreezeCtx<'_>) -> Flow {
            ctx.result = entity.can_freeze_default();
            Flow::Continue
        }
        fn make_stuck_in_block<E: Entity + ?Sized>(
            entity: &E,
            ctx: &mut MakeStuckInBlockCtx<'_>,
        ) -> Flow {
            entity.make_stuck_in_block_default(ctx.state, ctx.speed_multiplier);
            Flow::Continue
        }
        fn block_speed_factor<E: Entity + ?Sized>(
            entity: &E,
            ctx: &mut BlockSpeedFactorCtx<'_>,
        ) -> Flow {
            ctx.result = entity.block_speed_factor_default();
            Flow::Continue
        }
        Self {
            ride_tick: Pipeline::new([Stage::new(RIDE_TICK, ride_tick::<E>)]),
            check_fall_damage: Pipeline::new([Stage::new(
                CHECK_FALL_DAMAGE,
                check_fall_damage::<E>,
            )]),
            cause_fall_damage: Pipeline::new([Stage::new(
                CAUSE_FALL_DAMAGE,
                cause_fall_damage::<E>,
            )]),
            can_freeze: Pipeline::new([Stage::new(CAN_FREEZE, can_freeze::<E>)]),
            make_stuck_in_block: Pipeline::new([Stage::new(
                MAKE_STUCK_IN_BLOCK,
                make_stuck_in_block::<E>,
            )]),
            do_water_splash_effect: Pipeline::new([Stage::effect(
                DO_WATER_SPLASH_EFFECT,
                |entity: &E| entity.do_water_splash_effect_default(),
            )]),
            update_swimming: Pipeline::new([Stage::effect(UPDATE_SWIMMING, |entity: &E| {
                entity.update_swimming_default();
            })]),
            block_speed_factor: Pipeline::new([Stage::new(
                BLOCK_SPEED_FACTOR,
                block_speed_factor::<E>,
            )]),
            stop_riding: Pipeline::new([Stage::effect(STOP_RIDING, |entity: &E| {
                entity.remove_vehicle();
            })]),
            remove_vehicle: Pipeline::new([Stage::effect(REMOVE_VEHICLE, |entity: &E| {
                entity.remove_vehicle_default();
            })]),
            tick: Pipeline::new([Stage::effect(TICK, tick::<E>)]),
            base_tick: Pipeline::new([Stage::effect(BASE_TICK, base_tick::<E>)]),
            interact: Pipeline::new([Stage::new(INTERACT, interact::<E>)]),
            position_rider: Pipeline::new([Stage::new(POSITION_RIDER, position_rider::<E>)]),
            ..Self::empty()
        }
    }

    /// Vanilla `Entity`'s chains for a type whose `tick` never calls `super`.
    #[must_use]
    pub fn replacing_tick(stages: impl IntoIterator<Item = Stage<E, Tick>>) -> Self {
        Self {
            tick: Pipeline::new(stages),
            ..Self::new()
        }
    }
}

impl<E: Entity + ?Sized> Default for Definition<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// An entity type a [`Definition`] runs on: a concrete type, or `dyn Entity`
/// for vanilla `Entity`'s own chains.
pub trait DefinitionTarget: 'static {
    /// `entity` as this type, if it is one.
    fn typed(entity: &dyn Entity) -> Option<&Self>;
}

impl<E: Entity + DowncastType> DefinitionTarget for E {
    /// An entity's definition always comes from its own type, so a mismatch is
    /// a wiring bug.
    fn typed(entity: &dyn Entity) -> Option<&Self> {
        let typed = entity.downcast_ref::<Self>();
        if typed.is_none() {
            log::error!(
                "{} ran the definition of {}",
                entity.entity_type().key,
                Self::TYPE_KEY
            );
        }
        typed
    }
}

impl DefinitionTarget for dyn Entity {
    fn typed(entity: &dyn Entity) -> Option<&Self> {
        Some(entity)
    }
}

/// Vanilla `Entity`'s chains, for entities implemented without
/// `#[derive(Entity)]`, such as test fixtures.
pub static ENTITY_DEFINITION: LazyLock<Definition<dyn Entity>> = LazyLock::new(Definition::new);
