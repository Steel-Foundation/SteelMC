use rand::{Rng, RngExt};
use std::sync::Arc;
use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, BoolProperty};
use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;
use steel_registry::loot_table::LootContext;
use steel_registry::{
    sound_events, vanilla_blocks, vanilla_game_events, vanilla_items, vanilla_loot_tables,
};
use steel_utils::types::UpdateFlags;
use steel_utils::{BlockPos, BlockStateId, Direction};

use crate::behavior::blocks::vegetation::bonemealable::BonemealAction;
use crate::behavior::blocks::vegetation::growing_plant_head_block::GrowingPlantHeadBlock;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::{InteractionResult, InventoryAccess};
use crate::behavior::{block::BlockBehavior, blocks::vegetation::bonemealable::Bonemealable};
use crate::entity::{Entity, entity_loot_ref};
use crate::player::Player;
use crate::world::game_event_context::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

use super::BlockRef;

/// Vanilla `CaveVinesBlock` (head) survival.
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
    const fn growing_plant_head_block(&self) -> GrowingPlantHeadBlock {
        GrowingPlantHeadBlock::new(
            self.block,
            Direction::Down,
            false,
            0.1,
            &vanilla_blocks::CAVE_VINES_PLANT,
        )
        .with_update_body_after_converted_from_head(Self::update_body_after_converted_from_head)
        .with_update_grow_into_state(Self::update_grow_into_state)
    }

    fn update_body_after_converted_from_head(
        head_state: BlockStateId,
        body_state: BlockStateId,
    ) -> BlockStateId {
        body_state.set_value(&BERRIES, head_state.get_value(&BERRIES))
    }

    fn update_grow_into_state(state: BlockStateId, rng: &mut dyn Rng) -> BlockStateId {
        state.set_value(&BERRIES, rng.random::<f32>() < 0.11)
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
        let mut ctx = LootContext::new(&mut rng)
            .with_block_state(state)
            .with_interacting_entity(entity_loot_ref(source_entity));

        let items = vanilla_loot_tables::HARVEST_CAVE_VINE.get_random_items(&mut ctx);
        for item in items {
            world.pop_resource(pos, item);
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
    fn can_survive(&self, state: BlockStateId, world: &dyn LevelReader, pos: BlockPos) -> bool {
        self.growing_plant_head_block()
            .can_survive(state, world, pos)
    }
    fn is_randomly_ticking(&self, state: BlockStateId) -> bool {
        self.growing_plant_head_block().is_randomly_ticking(state)
    }
    fn random_tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_head_block()
            .random_tick(state, world, pos);
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
        self.growing_plant_head_block().update_shape(
            state,
            world,
            pos,
            direction,
            neighbor_pos,
            neighbor_state,
        )
    }
    fn tick(&self, state: BlockStateId, world: &Arc<World>, pos: BlockPos) {
        self.growing_plant_head_block().tick(state, world, pos);
    }

    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        self.growing_plant_head_block()
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

#[cfg(test)]
mod tests {
    use rand::{SeedableRng as _, rngs::StdRng};
    use steel_registry::test_support::init_test_registry;

    use super::*;
    use crate::test_support::TestLevel;

    #[test]
    fn head_conversion_preserves_berries() {
        init_test_registry();

        let behavior = CaveVinesBlock::new(&vanilla_blocks::CAVE_VINES);
        let state = vanilla_blocks::CAVE_VINES
            .default_state()
            .set_value(&BERRIES, true);
        let level = TestLevel::default();

        let converted = behavior.update_shape(
            state,
            &level,
            BlockPos::ZERO,
            Direction::Down,
            BlockPos::ZERO.below(),
            vanilla_blocks::CAVE_VINES_PLANT.default_state(),
        );

        assert_eq!(converted.get_block(), &vanilla_blocks::CAVE_VINES_PLANT);
        assert!(converted.get_value(&BERRIES));
    }

    #[test]
    fn grown_head_rolls_berries_independently() {
        init_test_registry();

        let state = vanilla_blocks::CAVE_VINES.default_state();
        let mut rng = StdRng::seed_from_u64(1);
        let berry_states = (0..256)
            .filter(|_| CaveVinesBlock::update_grow_into_state(state, &mut rng).get_value(&BERRIES))
            .count();

        assert!(berry_states > 0);
        assert!(berry_states < 256);
    }

    #[test]
    fn clone_item_is_glow_berries() {
        init_test_registry();

        let behavior = CaveVinesBlock::new(&vanilla_blocks::CAVE_VINES);
        let item = behavior
            .get_clone_item_stack(
                &vanilla_blocks::CAVE_VINES,
                vanilla_blocks::CAVE_VINES.default_state(),
                false,
            )
            .expect("cave vines have a vanilla clone item");

        assert!(item.is(&vanilla_items::GLOW_BERRIES));
    }
}
