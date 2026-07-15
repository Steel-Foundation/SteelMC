use rand::{Rng, RngExt};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::items::item::BlockHitResult;
use steel_registry::loot_table::LootContext;
use steel_registry::{sound_events, vanilla_blocks, vanilla_game_events, vanilla_loot_tables};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::bonemealable::BonemealAction;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{InteractionResult, InventoryAccess};
use crate::behavior::{block::BlockBehavior, blocks::vegetation::bonemealable::Bonemealable};
use crate::entity::Entity;
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, World};

use super::{BlockRef, default_surviving_state, growing_plant_can_survive};

/// Vanilla `CaveVinesBlock` (head) survival.
// TODO: Implement growth,  and shape updates.
#[block_behavior]
pub struct CaveVinesBlock {
    block: BlockRef,
}

const BERRIES: BoolProperty = BlockStateProperties::BERRIES;

impl CaveVinesBlock {
    /// Creates a new cave vines (head) block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
    /// Shared behavior use block between cave vine block and plant
    pub fn use_block(
        source_entity: &dyn Entity,
        state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> InteractionResult {
        if !state.get_value(&BERRIES) {
            return InteractionResult::Pass;
        }
        let mut rng = rand::rng();
        let mut ctx = LootContext::new(&mut rng).with_block_state(state);

        let items = vanilla_loot_tables::HARVEST_CAVE_VINE.get_random_items(&mut ctx);
        for item in items {
            world.drop_item_stack(pos, item);
        }
        let pitch = rng.random_range(0.8..1.2);
        world.play_sound(
            &sound_events::BLOCK_CAVE_VINES_PICK_BERRIES,
            SoundSource::Blocks,
            pos,
            1.0,
            pitch,
            None,
        );
        let new_state = state.set_value(&BERRIES, false);
        world.set_block(pos, new_state, UpdateFlags::UPDATE_CLIENTS);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(source_entity), Some(new_state)),
        );
        InteractionResult::Success
    }
}

impl BlockBehavior for CaveVinesBlock {
    fn can_survive(&self, _state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        growing_plant_can_survive(
            world,
            pos,
            Direction::Down,
            &vanilla_blocks::CAVE_VINES,
            &vanilla_blocks::CAVE_VINES_PLANT,
        )
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

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        default_surviving_state(self.block, self, context)
    }
    fn as_bonemealable(&self) -> Option<&dyn Bonemealable> {
        Some(self)
    }
}
impl Bonemealable for CaveVinesBlock {
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
