//! Pipeline entries: the overridable vanilla methods that run as pipelines,
//! with the arguments and result their stages share.

use std::sync::Arc;

use glam::DVec3;

use steel_utils::types::InteractionHand;

use crate::behavior::InteractionResult;
use crate::entity::component::pipeline::Entry;
use crate::entity::projectile::ProjectileHit;
use crate::entity::{Entity, EntitySpawnReason, SharedEntity, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::ClipHitResult;
use crate::world::World;

/// Vanilla `Entity.tick`.
pub struct Tick;

impl Entry for Tick {
    /// The shared handle of the entity being ticked.
    type Ctx<'a> = &'a SharedEntity;
}

/// Vanilla `Projectile.onHit`.
pub struct OnHit;

/// Context of one [`OnHit`] call.
pub struct OnHitCtx<'a> {
    /// The shared handle of the projectile.
    pub entity: &'a SharedEntity,
    /// What the projectile hit.
    pub hit: &'a ProjectileHit,
}

impl Entry for OnHit {
    type Ctx<'a> = OnHitCtx<'a>;
}

/// Vanilla `Projectile.onHitEntity`.
pub struct OnHitEntity;

/// Context of one [`OnHitEntity`] call.
pub struct OnHitEntityCtx<'a> {
    /// The shared handle of the projectile.
    pub entity: &'a SharedEntity,
    /// The entity hit.
    pub target: &'a SharedEntity,
    /// Where the projectile hit `target`.
    pub location: DVec3,
}

impl Entry for OnHitEntity {
    type Ctx<'a> = OnHitEntityCtx<'a>;
}

/// Vanilla `Projectile.onHitBlock`.
pub struct OnHitBlock;

/// Context of one [`OnHitBlock`] call.
pub struct OnHitBlockCtx<'a> {
    /// The shared handle of the projectile.
    pub entity: &'a SharedEntity,
    /// The block hit.
    pub hit: &'a ClipHitResult,
}

impl Entry for OnHitBlock {
    type Ctx<'a> = OnHitBlockCtx<'a>;
}

/// Vanilla `Projectile.canHitEntity`: stages refine [`CanHitEntityCtx::result`].
pub struct CanHitEntity;

/// Context of one [`CanHitEntity`] call.
pub struct CanHitEntityCtx<'a> {
    /// The entity the projectile may hit.
    pub target: &'a dyn Entity,
    /// Whether the projectile can hit `target`.
    pub result: bool,
}

impl Entry for CanHitEntity {
    type Ctx<'a> = CanHitEntityCtx<'a>;
}

/// The vanilla constructor chain of a mob, `Mob.registerGoals` included. Runs on
/// the fresh `MobBase` before the entity exists.
pub struct Construct;

impl Entry for Construct {
    type Ctx<'a> = ();
}

/// Vanilla `LivingEntity.aiStep`.
pub struct AiStep;

/// Context of one [`AiStep`] call.
pub struct AiStepCtx<'a> {
    /// The shared handle of the entity being ticked.
    pub entity: &'a SharedEntity,
    /// Movement performed by the living base stage.
    pub movement: Option<MoveResult>,
}

impl Entry for AiStep {
    type Ctx<'a> = AiStepCtx<'a>;
}

/// Vanilla `Mob.customServerAiStep`.
pub struct CustomServerAiStep;

impl Entry for CustomServerAiStep {
    type Ctx<'a> = ();
}

/// Vanilla `Mob.ate`, run after an eating goal consumes a block.
pub struct Ate;

impl Entry for Ate {
    type Ctx<'a> = ();
}

/// Vanilla `Mob.mobInteract`: the first stage that handles the interaction
/// sets [`MobInteractCtx::result`] and stops.
pub struct MobInteract;

/// Context of one [`MobInteract`] call.
pub struct MobInteractCtx<'a> {
    /// The interacting player.
    pub player: &'a Player,
    /// The hand the player interacts with.
    pub hand: InteractionHand,
    /// The interaction result; [`InteractionResult::Pass`] until handled.
    pub result: InteractionResult,
}

impl Entry for MobInteract {
    type Ctx<'a> = MobInteractCtx<'a>;
}

/// Vanilla `Mob.finalizeSpawn`.
pub struct FinalizeSpawn;

/// Context of one [`FinalizeSpawn`] call.
pub struct FinalizeSpawnCtx<'a> {
    /// The world the mob spawns into.
    pub world: &'a Arc<World>,
    /// Why the mob spawns.
    pub reason: EntitySpawnReason,
    /// Group data passed down the chain and returned to the spawner.
    pub group_data: Option<SpawnGroupData>,
}

impl Entry for FinalizeSpawn {
    type Ctx<'a> = FinalizeSpawnCtx<'a>;
}
