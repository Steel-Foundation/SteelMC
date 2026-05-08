//! `AnvilBlock` behavior.

use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_damage_types;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::block::{BlockBehavior, FallingEntityConfig};
use crate::behavior::context::BlockPlaceContext;
use crate::entity::Entity;
use crate::entity::damage::DamageSource;
use crate::entity::entities::falling_block::is_free;
use crate::world::World;

use super::{schedule_fall_tick, spawn_falling_entity};

/// Behavior for anvil, chipped anvil, and damaged anvil.
///
/// Anvils fall, damage entities on impact (2 HP per block fallen, max 40), and
/// use `FALLING_ANVIL` damage type. They also play a land/break sound via level
/// events (1031 = land, 1029 = break).
///
/// Vanilla: `AnvilBlock extends FallingBlock`.
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
}

impl BlockBehavior for AnvilBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Vanilla: AnvilBlock.getStateForPlacement → FACING = horizontalDirection.getClockWise()
        let facing = context.horizontal_direction.rotate_y_clockwise();
        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::HORIZONTAL_FACING, facing),
        )
    }

    fn on_place(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _old_state: BlockStateId,
        _moved_by_piston: bool,
    ) {
        schedule_fall_tick(world, pos, self.block);
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        schedule_fall_tick(world, pos, self.block);
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
        _entity: &dyn Entity,
    ) {
        // TODO: skip if entity.is_silent() — requires is_silent() on Entity trait
        world.level_event(1031, pos, 0, None);
    }

    fn on_broken_after_fall(&self, world: &Arc<World>, pos: BlockPos, _entity: &dyn Entity) {
        // TODO: skip if entity.is_silent() — requires is_silent() on Entity trait
        world.level_event(1029, pos, 0, None);
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
