//! Per-type definitions: the pipelines one entity type runs, built once.
//!
//! Every vanilla method that subclasses extend with `super` runs as a
//! pipeline here, so an entity type adds stages to its layers' chain instead
//! of calling the layer by hand. Methods overridden without `super` stay
//! hooks in the layer's props.

use std::sync::Arc;

use steel_utils::types::InteractionHand;
use steel_utils::{Downcast as _, DowncastType};

use crate::behavior::InteractionResult;
use crate::entity::component::entries::{
    AiStep, AiStepCtx, Ate, CanHitEntity, CanHitEntityCtx, Construct, CustomServerAiStep,
    FinalizeSpawn, FinalizeSpawnCtx, MobInteract, MobInteractCtx, OnHit, OnHitBlock, OnHitBlockCtx,
    OnHitCtx, OnHitEntity, OnHitEntityCtx, Tick,
};
use crate::entity::component::pipeline::{Pipeline, Stage, StageId};
use crate::entity::projectile::ProjectileHit;
use crate::entity::{Entity, EntitySpawnReason, MobBase, SharedEntity, SpawnGroupData};
use crate::physics::MoveResult;
use crate::player::Player;
use crate::world::ClipHitResult;
use crate::world::World;
use glam::DVec3;

/// The `Entity.tick` stage: vanilla's base entity tick, `Entity.baseTick`.
pub const TICK: StageId = StageId("entity.tick");

/// Pipelines of one entity type, built from its layer presets.
///
/// A layer's pipelines stay empty for entities without that layer.
pub struct Definition<E: 'static> {
    /// `Entity.tick`.
    pub tick: Pipeline<E, Tick>,
    /// `Projectile.onHit`.
    pub on_hit: Pipeline<E, OnHit>,
    /// `Projectile.onHitEntity`.
    pub on_hit_entity: Pipeline<E, OnHitEntity>,
    /// `Projectile.onHitBlock`.
    pub on_hit_block: Pipeline<E, OnHitBlock>,
    /// `Projectile.canHitEntity`.
    pub can_hit_entity: Pipeline<E, CanHitEntity>,
    /// The mob constructor chain, `Mob.registerGoals` included.
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

impl<E: Entity> Definition<E> {
    /// Vanilla `Entity`'s chains.
    #[must_use]
    pub fn new() -> Self {
        fn tick<E: Entity>(entity: &E) {
            entity.base_tick();
        }
        Self {
            tick: Pipeline::new([Stage::effect(TICK, tick::<E>)]),
            on_hit: Pipeline::new([]),
            on_hit_entity: Pipeline::new([]),
            on_hit_block: Pipeline::new([]),
            can_hit_entity: Pipeline::new([]),
            construct: Pipeline::new([]),
            ai_step: Pipeline::new([]),
            custom_server_ai_step: Pipeline::new([]),
            mob_interact: Pipeline::new([]),
            ate: Pipeline::new([]),
            finalize_spawn: Pipeline::new([]),
        }
    }
}

impl<E: Entity> Definition<E> {
    /// Vanilla `Entity`'s chains for a type whose `tick` never calls `super`.
    #[must_use]
    pub fn replacing_tick(stages: impl IntoIterator<Item = Stage<E, Tick>>) -> Self {
        Self {
            tick: Pipeline::new(stages),
            ..Self::new()
        }
    }
}

impl<E: Entity> Default for Definition<E> {
    fn default() -> Self {
        Self::new()
    }
}

/// A [`Definition`] reached from code that only holds `&dyn Entity`.
///
/// Implemented once for every definition, so entity types never forward
/// entry points by hand.
pub trait DefinitionEntries: Send + Sync {
    /// Runs `tick` for `entity`.
    fn tick(&self, entity: &SharedEntity);

    /// Runs `onHit` for `projectile`.
    fn on_hit(&self, projectile: &SharedEntity, hit: &ProjectileHit);

    /// Runs `onHitEntity` for `projectile` hitting `target` at `location`.
    fn on_hit_entity(&self, projectile: &SharedEntity, target: &SharedEntity, location: DVec3);

    /// Runs `onHitBlock` for `projectile`.
    fn on_hit_block(&self, projectile: &SharedEntity, hit: &ClipHitResult);

    /// Runs `canHitEntity`: whether `projectile` can hit `target`.
    fn can_hit_entity(&self, projectile: &dyn Entity, target: &dyn Entity) -> bool;

    /// Runs the mob constructor chain on a fresh `mob_base`.
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

impl<E: Entity + DowncastType> Definition<E> {
    /// `entity` as this definition's entity type. An entity's definition
    /// always comes from its own type, so a mismatch is a wiring bug.
    fn typed(entity: &dyn Entity) -> Option<&E> {
        let typed = entity.downcast_ref::<E>();
        if typed.is_none() {
            log::error!(
                "{} ran the definition of {}",
                entity.entity_type().key,
                E::TYPE_KEY
            );
        }
        typed
    }
}

impl<E: Entity + DowncastType> DefinitionEntries for Definition<E> {
    fn tick(&self, entity: &SharedEntity) {
        if let Some(typed) = Self::typed(entity.as_ref()) {
            let mut ctx = entity;
            self.tick.run(typed, &mut ctx);
        }
    }

    fn on_hit(&self, projectile: &SharedEntity, hit: &ProjectileHit) {
        if let Some(typed) = Self::typed(projectile.as_ref()) {
            let mut ctx = OnHitCtx {
                entity: projectile,
                hit,
            };
            self.on_hit.run(typed, &mut ctx);
        }
    }

    fn on_hit_entity(&self, projectile: &SharedEntity, target: &SharedEntity, location: DVec3) {
        if let Some(typed) = Self::typed(projectile.as_ref()) {
            let mut ctx = OnHitEntityCtx {
                entity: projectile,
                target,
                location,
            };
            self.on_hit_entity.run(typed, &mut ctx);
        }
    }

    fn on_hit_block(&self, projectile: &SharedEntity, hit: &ClipHitResult) {
        if let Some(typed) = Self::typed(projectile.as_ref()) {
            let mut ctx = OnHitBlockCtx {
                entity: projectile,
                hit,
            };
            self.on_hit_block.run(typed, &mut ctx);
        }
    }

    fn can_hit_entity(&self, projectile: &dyn Entity, target: &dyn Entity) -> bool {
        let Some(typed) = Self::typed(projectile) else {
            return false;
        };
        let mut ctx = CanHitEntityCtx {
            target,
            result: false,
        };
        self.can_hit_entity.run(typed, &mut ctx);
        ctx.result
    }

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

/// Vanilla `Entity`'s chains for entities implemented without
/// `#[derive(Entity)]`, such as test fixtures.
pub struct DefaultDefinition;

impl DefinitionEntries for DefaultDefinition {
    fn tick(&self, entity: &SharedEntity) {
        entity.base_tick();
    }

    fn on_hit(&self, _projectile: &SharedEntity, _hit: &ProjectileHit) {}

    fn on_hit_entity(&self, _projectile: &SharedEntity, _target: &SharedEntity, _location: DVec3) {}

    fn on_hit_block(&self, _projectile: &SharedEntity, _hit: &ClipHitResult) {}

    fn can_hit_entity(&self, _projectile: &dyn Entity, _target: &dyn Entity) -> bool {
        false
    }

    fn construct(&self, _mob_base: &MobBase) {}

    fn ai_step(&self, _mob: &dyn Entity, _entity: &SharedEntity) -> Option<MoveResult> {
        None
    }

    fn custom_server_ai_step(&self, _mob: &dyn Entity) {}

    fn ate(&self, _mob: &dyn Entity) {}

    fn mob_interact(
        &self,
        _mob: &dyn Entity,
        _player: &Player,
        _hand: InteractionHand,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    fn finalize_spawn(
        &self,
        _mob: &dyn Entity,
        _world: &Arc<World>,
        _reason: EntitySpawnReason,
        group_data: Option<SpawnGroupData>,
    ) -> Option<SpawnGroupData> {
        group_data
    }
}
