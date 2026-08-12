//! Brushable block behavior for suspicious sand and suspicious gravel.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, IntProperty};
use steel_registry::sound_event::SoundEventRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::Downcast as _;
use steel_utils::{BlockPos, BlockStateId};

use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext, BrushableData};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::BrushableBlockEntity;
use crate::world::{ScheduledTickAccess, World};

/// Vanilla archaeology block behavior for suspicious sand and suspicious gravel.
#[block_behavior]
pub struct BrushableBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "turns_into")]
    turns_into: BlockRef,
    #[json_arg(sound_events, json = "brush_sound")]
    brush_sound: SoundEventRef,
    #[json_arg(sound_events, json = "brush_completed_sound")]
    brush_completed_sound: SoundEventRef,
}

const DUSTED: &IntProperty = &BlockStateProperties::DUSTED;

impl BrushableBlock {
    /// Creates a brushable block behavior from extracted vanilla block arguments.
    #[must_use]
    pub const fn new(
        block: BlockRef,
        turns_into: BlockRef,
        brush_sound: SoundEventRef,
        brush_completed_sound: SoundEventRef,
    ) -> Self {
        Self {
            block,
            turns_into,
            brush_sound,
            brush_completed_sound,
        }
    }
}

impl BlockBehavior for BrushableBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(DUSTED, 0),
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
        world.schedule_block_tick_default(pos, self.block, 2);
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
        let _ = world.schedule_block_tick_default(pos, self.block, 2);
        state
    }

    fn tick(&self, _state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return;
        };
        let Some(brushable) = block_entity.downcast_ref::<BrushableBlockEntity>() else {
            return;
        };
        let mutation = brushable.check_reset(world);
        mutation.apply(world, pos);

        // TODO: Fall without dropping once Steel implements vanilla falling-block entities.
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BRUSHABLE_BLOCK,
            level,
            pos,
            state,
        ))
    }

    fn should_keep_block_entity(&self, old_state: BlockStateId, new_state: BlockStateId) -> bool {
        old_state.get_block() == new_state.get_block()
    }

    fn brushable_data(&self, _state: BlockStateId) -> Option<BrushableData> {
        Some(BrushableData {
            turns_into: self.turns_into,
            brush_sound: self.brush_sound,
            brush_completed_sound: self.brush_completed_sound,
        })
    }
}
