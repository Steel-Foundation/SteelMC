use rand::Rng;
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_blocks;
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::CaveVinesBlock;
use crate::behavior::blocks::vegetation::bonemealable::{BonemealAction, Bonemealable};
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{InteractionResult, InventoryAccess};
use crate::player::Player;
use crate::world::LevelReader;
use crate::{behavior::block::BlockBehavior, world::World};

use super::{BlockRef, default_surviving_state, growing_plant_can_survive};

/// Vanilla `CaveVinesPlantBlock` (body) survival.
// TODO: Implement shape updates.
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
}

impl BlockBehavior for CaveVinesPlantBlock {
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

    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            Direction::Down,
            &vanilla_blocks::CAVE_VINES,
            &vanilla_blocks::CAVE_VINES_PLANT,
        )
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
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
