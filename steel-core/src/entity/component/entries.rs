//! Pipeline entries: the overridable vanilla methods that run as pipelines,
//! with the arguments and result their stages share.

use std::sync::Arc;

use steel_utils::types::InteractionHand;

use crate::behavior::InteractionResult;
use crate::entity::component::pipeline::Entry;
use crate::entity::{EntitySpawnReason, SharedEntity, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

/// The vanilla constructor chain of a mob, `Mob.registerGoals` included. Runs on
/// the fresh [`MobBase`](crate::entity::MobBase) before the entity exists.
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
