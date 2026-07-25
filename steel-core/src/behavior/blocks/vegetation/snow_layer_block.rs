use std::sync::Arc;

use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, IntProperty};
use steel_registry::blocks::shapes;
use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::block::BlockBehavior;
use crate::behavior::blocks::vegetation::vegetation_block::survival_update_shape;
use crate::behavior::context::BlockPlaceContext;
use crate::chunk::light::LightLayer;
use crate::entity::ai::path::PathComputationType;
use crate::fluid::fluid_state_to_block;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `SnowLayerBlock` survival.
///
/// 1. If below is in `cannot_support_snow_layer`, false.
/// 2. If below is in `support_override_snow_layer`, true.
/// 3. Otherwise: below's collision shape has a full UP face, or below is snow
///    with `LAYERS = 8`.
#[block_behavior]
pub struct SnowLayerBlock {
    block: BlockRef,
}

const LAYERS: IntProperty = BlockStateProperties::LAYERS;

impl SnowLayerBlock {
    /// Creates a new snow layer block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for SnowLayerBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        let below = world.get_block_state(pos.below());
        let below_block = below.get_block();

        if below_block.has_tag(&BlockTag::CANNOT_SUPPORT_SNOW_LAYER) {
            return false;
        }

        if below_block.has_tag(&BlockTag::SUPPORT_OVERRIDE_SNOW_LAYER) {
            return true;
        }

        if shapes::is_offset_face_full(below.get_collision_shape_at(pos.below()), Direction::Up) {
            return true;
        }

        // Below is another snow layer fully filled (LAYERS == 8).
        below_block == self.block && below.get_value(&LAYERS) == 8
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
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        if world.light_value_at(LightLayer::Block, pos) > 11 {
            world.drop_resources(state, pos);
            world.set_block(
                pos,
                fluid_state_to_block(state.get_fluid_state()),
                UpdateFlags::UPDATE_ALL,
            );
        }
    }
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        let layers = state.get_value(&LAYERS);
        if !context.with_item(|item| item.item() == REGISTRY.items.by_block(state.get_block()))
            || layers >= 8
        {
            return layers == 1;
        }
        if context.replaces_clicked_block() {
            return context.clicked_face() == Direction::Up;
        }
        true
    }
    fn is_pathfindable(&self, state: BlockStateId, computation_type: PathComputationType) -> bool {
        computation_type == PathComputationType::Land && state.get_value(&LAYERS) < 5
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let pos = if context.replaces_clicked_block() {
            context.hit_pos()
        } else {
            context.place_pos()
        };
        let state = context.world.get_block_state(pos);
        if state.get_block() == self.block {
            let layers = state.get_value(&LAYERS);
            return Some(state.set_value(&LAYERS, 8.min(layers + 1)));
        }
        Some(self.block.default_state())
    }
}
