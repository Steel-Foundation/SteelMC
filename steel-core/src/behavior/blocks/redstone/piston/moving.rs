//! Vanilla transient moving-piston block behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::item_stack::ItemStack;
use steel_registry::{vanilla_block_entity_types, vanilla_blocks};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::{
    BLOCK_BEHAVIORS, BlockBehavior, BlockCollisionBoxes, BlockCollisionContext,
    BlockEntityCreation, BlockHitResult, BlockLootContext, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::block_entity::{BlockEntityTicker, entities::PistonMovingBlockEntity};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::{LevelReader, World};

#[block_behavior]
/// Vanilla transient moving-piston block.
pub struct MovingPistonBlock;

impl MovingPistonBlock {
    /// Creates moving-piston behavior.
    #[must_use]
    pub const fn new(_block: BlockRef) -> Self {
        Self
    }
}

impl BlockBehavior for MovingPistonBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(vanilla_blocks::MOVING_PISTON.default_state())
    }

    fn new_block_entity(
        &self,
        _level: Weak<World>,
        _pos: BlockPos,
        _state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::NoEntity
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::PISTON,
        )
    }

    fn get_collision_boxes(
        &self,
        _state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
        _context: BlockCollisionContext,
    ) -> BlockCollisionBoxes {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return BlockCollisionBoxes::new();
        };
        block_entity
            .downcast_ref::<PistonMovingBlockEntity>()
            .map_or_else(BlockCollisionBoxes::new, |piston| {
                piston.collision_boxes(world, pos)
            })
    }

    fn get_block_support_boxes(
        &self,
        state: BlockStateId,
        world: &dyn LevelReader,
        pos: BlockPos,
    ) -> BlockCollisionBoxes {
        self.get_collision_boxes(state, world, pos, BlockCollisionContext::empty())
    }

    fn destroy(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        let base_pos = pos.relative(state.get_value(&BlockStateProperties::FACING).opposite());
        let base_state = world.get_block_state(base_pos);
        let behavior = BLOCK_BEHAVIORS.get_behavior(base_state.get_block());
        if behavior.is_piston_base() && base_state.get_value(&BlockStateProperties::EXTENDED) {
            world.remove_block(base_pos, false);
        }
    }

    fn get_drops(
        &self,
        _state: BlockStateId,
        context: &BlockLootContext<'_>,
    ) -> Option<Vec<ItemStack>> {
        let Some(block_entity) = context.world().get_block_entity(context.pos()) else {
            return Some(Vec::new());
        };
        let Some(piston) = block_entity.downcast_ref::<PistonMovingBlockEntity>() else {
            return Some(Vec::new());
        };
        let moved_state = piston.moved_state();
        Some(context.get_drops(moved_state))
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if world.get_block_entity(pos).is_none() {
            world.remove_block(pos, false);
            InteractionResult::Consume
        } else {
            InteractionResult::Pass
        }
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::empty())
    }

    fn is_pathfindable(&self, _state: BlockStateId, _type: PathComputationType) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::test_support::fresh_test_world;

    #[test]
    fn moving_piston_selects_only_the_piston_block_entity_ticker() {
        init_test_registry();
        let world = fresh_test_world("moving_piston_ticker");
        let behavior = MovingPistonBlock::new(&vanilla_blocks::MOVING_PISTON);
        let state = vanilla_blocks::MOVING_PISTON.default_state();

        assert!(
            behavior
                .get_block_entity_ticker(&world, state, &vanilla_block_entity_types::PISTON)
                .is_some()
        );
        assert!(
            behavior
                .get_block_entity_ticker(&world, state, &vanilla_block_entity_types::CHEST)
                .is_none()
        );
    }
}
