use std::sync::{Arc, Weak};
use steel_macros::block_behavior;
use steel_registry::{blocks::{BlockRef, block_state_ext::BlockStateExt, properties::BlockStateProperties}, vanilla_block_entity_types};
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;
use crate::{
    behavior::{
        InventoryAccess,
        block::{BlockBehavior, BlockEntityCreation},
        context::{BlockHitResult, InteractionResult},
    },
    block_entity::BLOCK_ENTITIES,
    inventory::{lock::ContainerRef, menu::kinds::furnace},
    player::Player,
    world::World,
};

#[block_behavior]
pub struct FurnaceBlock {
    block: BlockRef,
}

impl FurnaceBlock {
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for FurnaceBlock {
    fn get_state_for_placement(
        &self,
        context: &crate::behavior::context::BlockPlaceContext<'_>,
    ) -> Option<BlockStateId> {
        let facing = context.horizontal_direction().opposite();
        Some(self.block.default_state().set_value(&BlockStateProperties::HORIZONTAL_FACING, facing))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: steel_registry::block_entity_type::BlockEntityTypeRef,
    ) -> Option<crate::block_entity::BlockEntityTicker> {
        crate::block_entity::BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::FURNACE,
        )
    }

    fn use_without_item(
        &self,
        _: BlockStateId,
        _: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _: &BlockHitResult,
        _: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(entity) = player.get_world().get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        let Some(container) = ContainerRef::from_block_entity(entity) else {
            return InteractionResult::Pass;
        };

        let inv = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_FURNACE.msg()),
            move |ctx| furnace(inv, ctx.container_id, container),
        );

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::FURNACE,
            level,
            pos,
            state,
        ))
    }
}
