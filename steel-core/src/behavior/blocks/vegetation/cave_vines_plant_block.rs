use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;
use steel_registry::{vanilla_blocks, vanilla_items};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::CaveVinesBlock;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::blocks::vegetation::growing_plant_body_block::GrowingPlantBodyBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{InteractionResult, InventoryAccess};
use crate::player::Player;
use crate::world::{LevelReader, ScheduledTickAccess};
use crate::{behavior::block::BlockBehavior, world::World};

use super::BlockRef;

/// Vanilla `CaveVinesPlantBlock` (body) survival.
#[block_behavior]
pub struct CaveVinesPlantBlock {
    block: BlockRef,
}

const BERRIES: BoolProperty = BlockStateProperties::BERRIES;

impl CaveVinesPlantBlock {
    /// Creates a new cave vines plant (body) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }

    const fn growing_plant_body_block(&self) -> GrowingPlantBodyBlock {
        GrowingPlantBodyBlock::new(
            self.block,
            Direction::Down,
            false,
            &vanilla_blocks::CAVE_VINES,
        )
        .with_update_head_after_converted_from_body(Self::update_head_after_converted_from_body)
    }

    fn update_head_after_converted_from_body(
        body_state: BlockStateId,
        head_state: BlockStateId,
    ) -> BlockStateId {
        head_state.set_value(&BERRIES, body_state.get_value(&BERRIES))
    }
}

impl BlockBehavior for CaveVinesPlantBlock {
    fn can_be_replaced(&self, state: BlockStateId, context: &BlockPlaceContext<'_>) -> bool {
        self.growing_plant_body_block()
            .can_be_replaced(state, context)
    }

    fn use_without_item(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        CaveVinesBlock::use_block(player, state, world, pos)
    }

    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.growing_plant_body_block()
            .can_survive(state, world, pos)
    }
    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        neighbor_pos: BlockPos,
        neighbor_state: BlockStateId,
    ) -> BlockStateId {
        self.growing_plant_body_block().update_shape(
            state,
            world,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        )
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_body_block().tick(state, world, pos);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.growing_plant_body_block()
            .get_state_for_placement(context)
    }

    fn get_clone_item_stack(
        &self,
        _block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        Some(ItemStack::new(&vanilla_items::GLOW_BERRIES))
    }
}
impl Bonemealable for CaveVinesPlantBlock {
    fn is_valid_bonemeal_target(
        &self,
        state: BlockStateId,
        _world: &dyn LevelReader,
        _pos: BlockPos,
    ) -> bool {
        !state.get_value(&BERRIES)
    }

    fn is_bonemeal_success(
        &self,
        _state: BlockStateId,
        _world: &Arc<World>,
        _rng: &mut dyn Rng,
        _pos: BlockPos,
    ) -> bool {
        true
    }

    fn perform_bonemeal(
        &self,
        state: BlockStateId,
        world: &Arc<World>,
        _rng: &mut dyn Rng,
        pos: BlockPos,
    ) {
        world.set_block(
            pos,
            state.set_value(&BERRIES, true),
            UpdateFlags::UPDATE_CLIENTS,
        );
    }

    fn bonemeal_action_type(&self) -> BonemealAction {
        BonemealAction::Grower
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::test_support::TestLevel;

    #[test]
    fn body_conversion_preserves_berries() {
        init_test_registry();

        let behavior = CaveVinesPlantBlock::new(&vanilla_blocks::CAVE_VINES_PLANT);
        let state = vanilla_blocks::CAVE_VINES_PLANT
            .default_state()
            .set_value(&BERRIES, true);
        let level = TestLevel::default();

        let converted = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::AIR.default_state(),
        );

        assert_eq!(converted.get_block(), &vanilla_blocks::CAVE_VINES);
        assert!(converted.get_value(&BERRIES));
    }

    #[test]
    fn clone_item_is_glow_berries() {
        init_test_registry();

        let behavior = CaveVinesPlantBlock::new(&vanilla_blocks::CAVE_VINES_PLANT);
        let item = behavior
            .get_clone_item_stack(
                &vanilla_blocks::CAVE_VINES_PLANT,
                vanilla_blocks::CAVE_VINES_PLANT.default_state(),
                false,
            )
            .expect("cave vines plants have a vanilla clone item");

        assert!(item.is(&vanilla_items::GLOW_BERRIES));
    }
}
