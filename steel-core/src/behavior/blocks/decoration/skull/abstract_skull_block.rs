use crate::behavior::{BlockBehavior, BlockEntityCreation, BlockPlaceContext};
use crate::block_entity::BLOCK_ENTITIES;
use crate::entity::ai::path::PathComputationType;
use crate::world::{SignalGetter, World};
use std::sync::{Arc, Weak};
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::vanilla_block_entity_types;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId};

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

    fn handle_skull_neighbor_changed(
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

    fn is_skull_pathfindable(
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
}
