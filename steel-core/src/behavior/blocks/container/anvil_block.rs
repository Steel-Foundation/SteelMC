//! `AnvilBlock` behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::level_events;
use steel_registry::vanilla_blocks;
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, FallingEntityConfig};
use crate::behavior::blocks::falling::{schedule_fall_tick, spawn_falling_entity};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::entity::entities::falling_block::is_free;
use crate::world::{ScheduledTickAccess, World};

/// Behavior for anvil, chipped anvil, and damaged anvil.
///
/// Anvils fall, damage entities on impact (2 HP per block fallen, max 40), and
/// use `FALLING_ANVIL` damage type. They also play a land/break sound via level
/// events (`SOUND_ANVIL_LAND` on land, `SOUND_ANVIL_BROKEN` on break).
#[block_behavior(class = "AnvilBlock")]
pub struct AnvilBlock {
    block: BlockRef,
}

impl AnvilBlock {
    /// Creates a new `AnvilBlock` behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    /// Returns the next damage level block state: ANVIL → CHIPPED → DAMAGED → None (destroyed).
    #[must_use]
    pub fn damage(state: BlockStateId) -> Option<BlockStateId> {
        let block = state.get_block();
        let facing = state.try_get_value(&BlockStateProperties::HORIZONTAL_FACING);
        let with_facing = |new_state: BlockStateId| match facing {
            Some(f) => new_state.set_value(&BlockStateProperties::HORIZONTAL_FACING, f),
            None => new_state,
        };
        if block == &vanilla_blocks::ANVIL {
            Some(with_facing(vanilla_blocks::CHIPPED_ANVIL.default_state()))
        } else if block == &vanilla_blocks::CHIPPED_ANVIL {
            Some(with_facing(vanilla_blocks::DAMAGED_ANVIL.default_state()))
        } else {
            None
        }
    }
}

impl BlockBehavior for AnvilBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction.rotate_y_clockwise(),
        ))
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_fall_tick(world, pos, self.block, 2);
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
        schedule_fall_tick(world, pos, self.block, 2);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let below = world.get_block_state(pos.below());
        if is_free(below) && pos.y() >= world.get_min_y() {
            spawn_falling_entity(world, pos, state);
        }
    }

    fn falling_entity_config(&self) -> FallingEntityConfig {
        FallingEntityConfig {
            hurt_entities: true,
            fall_damage_per_distance: 2.0,
            fall_damage_max: 40,
        }
    }

    fn on_land(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        _placed_state: BlockStateId,
        _replaced_state: BlockStateId,
        entity: &dyn Entity,
    ) {
        if !entity.is_silent() {
            world.level_event(level_events::SOUND_ANVIL_LAND, pos, 0, None);
        }
    }

    fn on_broken_after_fall(&self, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if !entity.is_silent() {
            world.level_event(level_events::SOUND_ANVIL_BROKEN, pos, 0, None);
        }
    }

    fn fall_damage_source(&self, direct_entity_id: i32) -> DamageSource {
        DamageSource {
            damage_type: &vanilla_damage_types::FALLING_ANVIL,
            direct_entity_id: Some(direct_entity_id),
            causing_entity_id: None,
            source_position: None,
        }
    }
}
