//! Vanilla `SculkShriekerBlock`.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::value_providers::IntProvider;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::{schedule_water_tick_if_waterlogged, try_drop_experience};
use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext};
use crate::block_entity::entities::SculkShriekerBlockEntity;
use crate::block_entity::BlockEntityTicker;
use crate::entity::Entity;
use crate::world::{ScheduledTickAccess, World};
use steel_utils::Downcast;

const SHRIEKING: &BoolProperty = &BlockStateProperties::SHRIEKING;
const WATERLOGGED: &BoolProperty = &BlockStateProperties::WATERLOGGED;
const EXPERIENCE: IntProvider = IntProvider::Constant(5);

/// Vanilla `SculkShriekerBlock`.
#[block_behavior]
pub struct SculkShriekerBlock {
    block: BlockRef,
}

impl SculkShriekerBlock {
    /// Creates sculk-shrieker behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SculkShriekerBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(WATERLOGGED, context.is_water_source()),
        )
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
        schedule_water_tick_if_waterlogged(state, world, pos);
        state
    }

    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if !state.get_value(SHRIEKING) {
            return;
        }
        world.set_block(
            pos,
            state.set_value(SHRIEKING, false),
            UpdateFlags::UPDATE_CLIENTS,
        );
        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(shrieker) = block_entity.downcast_ref::<SculkShriekerBlockEntity>()
        {
            shrieker.try_respond(world);
        }
    }

    fn step_on(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos, entity: &dyn Entity) {
        if let Some(block_entity) = world.get_block_entity(pos)
            && let Some(shrieker) = block_entity.downcast_ref::<SculkShriekerBlockEntity>()
        {
            shrieker.try_shriek_from_entity(world, entity);
        }
        self.default_step_on(state, world, pos, entity);
    }

    fn spawn_after_break(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        tool: &ItemStack,
        drop_experience: bool,
    ) {
        if drop_experience {
            try_drop_experience(world, pos, tool, &EXPERIENCE);
        }
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::Created(Arc::new(SculkShriekerBlockEntity::new(pos, state, level)))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::SCULK_SHRIEKER,
        )
    }
}
