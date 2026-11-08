//! Per-type definitions: the pipelines one entity type runs, built once.

use std::sync::Arc;

use steel_utils::types::InteractionHand;
use steel_utils::{Downcast as _, DowncastType};

use crate::behavior::InteractionResult;
use crate::entity::component::entries::{
    AiStep, AiStepCtx, Ate, Construct, CustomServerAiStep, FinalizeSpawn, FinalizeSpawnCtx,
    MobInteract, MobInteractCtx,
};
use crate::entity::component::pipeline::Pipeline;
use crate::entity::{Entity, EntitySpawnReason, MobBase, SharedEntity, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::World;

/// Pipelines of one mob type, built from its layer presets.
pub struct MobDefinition<E: 'static> {
    /// The constructor chain, `Mob.registerGoals` included.
    pub construct: Pipeline<MobBase, Construct>,
    /// `LivingEntity.aiStep`.
    pub ai_step: Pipeline<E, AiStep>,
    /// `Mob.customServerAiStep`.
    pub custom_server_ai_step: Pipeline<E, CustomServerAiStep>,
    /// `Mob.mobInteract`.
    pub mob_interact: Pipeline<E, MobInteract>,
    /// `Mob.ate`.
    pub ate: Pipeline<E, Ate>,
    /// `Mob.finalizeSpawn`.
    pub finalize_spawn: Pipeline<E, FinalizeSpawn>,
}

/// A [`MobDefinition`] reached from code that only holds `&dyn Entity`.
///
/// Implemented once for every definition, so entity types never forward
/// entry points by hand.
pub trait MobEntries: Send + Sync {
    /// Runs the constructor chain on a fresh `mob_base`.
    fn construct(&self, mob_base: &MobBase);

    /// Runs `aiStep` for `mob`, returning the movement it performed.
    fn ai_step(&self, mob: &dyn Entity, entity: &SharedEntity) -> Option<MoveResult>;

    /// Runs `customServerAiStep` for `mob`.
    fn custom_server_ai_step(&self, mob: &dyn Entity);

    /// Runs `ate` for `mob`.
    fn ate(&self, mob: &dyn Entity);

    /// Runs `mobInteract` for `mob`.
    fn mob_interact(
        &self,
        mob: &dyn Entity,
        player: &Player,
        hand: InteractionHand,
    ) -> InteractionResult;

    /// Runs `finalizeSpawn` for `mob`, returning the group data for the
    /// next group member.
    fn finalize_spawn(
        &self,
        mob: &dyn Entity,
        world: &Arc<World>,
        reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData>;
}

impl<E: Entity + DowncastType> MobDefinition<E> {
    /// `mob` as this definition's entity type. A mob's definition always
    /// comes from its own properties, so a mismatch is a wiring bug.
    fn typed(mob: &dyn Entity) -> Option<&E> {
        let typed = mob.downcast_ref::<E>();
        if typed.is_none() {
            log::error!(
                "{} ran the definition of {}",
                mob.entity_type().key,
                E::TYPE_KEY
            );
        }
        typed
    }
}

impl<E: Entity + DowncastType> MobEntries for MobDefinition<E> {
    fn construct(&self, mob_base: &MobBase) {
        self.construct.run(mob_base, &mut ());
    }

    fn ai_step(&self, mob: &dyn Entity, entity: &SharedEntity) -> Option<MoveResult> {
        let mob = Self::typed(mob)?;
        let mut ctx = AiStepCtx {
            entity,
            movement: None,
        };
        self.ai_step.run(mob, &mut ctx);
        ctx.movement
    }

    fn custom_server_ai_step(&self, mob: &dyn Entity) {
        if let Some(mob) = Self::typed(mob) {
            self.custom_server_ai_step.run(mob, &mut ());
        }
    }

    fn ate(&self, mob: &dyn Entity) {
        if let Some(mob) = Self::typed(mob) {
            self.ate.run(mob, &mut ());
        }
    }

    fn mob_interact(
        &self,
        mob: &dyn Entity,
        player: &Player,
        hand: InteractionHand,
    ) -> InteractionResult {
        let Some(mob) = Self::typed(mob) else {
            return InteractionResult::Pass;
        };
        let mut ctx = MobInteractCtx {
            player,
            hand,
            result: InteractionResult::Pass,
        };
        self.mob_interact.run(mob, &mut ctx);
        ctx.result
    }

    fn finalize_spawn(
        &self,
        mob: &dyn Entity,
        world: &Arc<World>,
        reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        let Some(mob) = Self::typed(mob) else {
            return group_data;
        };
        let mut ctx = FinalizeSpawnCtx {
            world,
            reason,
            group_data,
        };
        self.finalize_spawn.run(mob, &mut ctx);
        ctx.group_data
    }
}
