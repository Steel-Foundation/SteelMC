use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_registry::{vanilla_damage_types, vanilla_mob_effects};
use steel_utils::types::Difficulty;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::vegetation_block::survival_update_shape;
use crate::behavior::context::BlockPlaceContext;
use crate::entity::damage::DamageSource;
use crate::entity::{Entity, InsideBlockEffectCollector, MobEffectInstance};
use crate::world::{LevelReader, ScheduledTickAccess};
use crate::{behavior::block::BlockBehavior, world::World};

use super::{BlockRef, default_surviving_state, survives_on_tag};

/// Behavior for wither roses.
#[block_behavior]
pub struct WitherRoseBlock {
    block: BlockRef,
}

impl WitherRoseBlock {
    /// Creates a new wither rose block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for WitherRoseBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        survives_on_tag(world, pos, &BlockTag::SUPPORTS_WITHER_ROSE)
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        survival_update_shape(self, state, world, pos)
    }
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
    fn entity_inside(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        _pos: BlockPos,
        entity: &dyn Entity,
        _effect_collector: &mut InsideBlockEffectCollector,
        _is_precise: bool,
    ) {
        if world.difficulty() == Difficulty::Peaceful {
            return;
        }
        let Some(living_entity) = entity.as_living_entity() else {
            return;
        };
        if living_entity.is_invulnerable_to(
            world,
            &DamageSource::environment(&vanilla_damage_types::WITHER),
        ) {
            return;
        }
        living_entity.add_mob_effect(MobEffectInstance::with_duration(
            vanilla_mob_effects::WITHER,
            40,
            0,
        ));
    }
}
