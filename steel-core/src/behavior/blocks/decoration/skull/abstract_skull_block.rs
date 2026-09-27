use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockLootContext, BlockPlaceContext};
use crate::block_entity::entities::SkullBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::entity::ai::path::PathComputationType;
use crate::world::{SignalGetter, World};
use std::sync::{Arc, Weak};
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::{REGISTRY, vanilla_block_entity_types};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Downcast};

const POWERED: &BoolProperty = &BlockStateProperties::POWERED;

/// Shared server behavior inherited from vanilla's `AbstractSkullBlock`.
pub(super) trait AbstractSkullBlock: BlockBehavior {
    #[must_use]
    fn state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId>;

    fn default_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.state_for_placement(context).map(|state| {
            state.set_value(
                POWERED,
                context.world.has_neighbor_signal(context.place_pos()),
            )
        })
    }

    fn default_handle_neighbor_changed(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _source_block: BlockRef,
        _moved_by_piston: bool,
    ) {
        let signal: bool = world.has_neighbor_signal(pos);
        if signal != state.get_value(POWERED) {
            world.set_block(
                pos,
                state.set_value(POWERED, signal),
                UpdateFlags::UPDATE_CLIENTS,
            );
        }
    }

    fn default_is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }

    fn new_skull_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::SKULL,
            level,
            pos,
            state,
        ))
    }

    fn default_get_clone_item_stack(
        &self,
        block: BlockRef,
        state: BlockStateId,
        block_entity: Option<SharedBlockEntity>,
        include_data: bool,
    ) -> Option<ItemStack> {
        if !include_data {
            return Some(ItemStack::new(REGISTRY.items.by_block(block)));
        }

        let block_entity = block_entity?;
        let skull_block_entity = block_entity.downcast_ref::<SkullBlockEntity>()?;

        Some(skull_block_entity.skull_as_item(state))
    }

    fn default_get_drops(
        &self,
        state: BlockStateId,
        context: &BlockLootContext,
    ) -> Option<Vec<ItemStack>> {
        let block_entity = context.block_entity()?;
        let skull_block_entity = block_entity.downcast_ref::<SkullBlockEntity>()?;

        let item = skull_block_entity.skull_as_item(state);
        Some(vec![item])
    }
}
