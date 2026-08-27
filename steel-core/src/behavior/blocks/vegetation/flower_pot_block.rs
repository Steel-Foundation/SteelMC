//! Vanilla `FlowerPotBlock` behavior.
//!
//! Empty pots accept a pottable plant; filled pots return the plant on empty-hand
//! use. Eyeblossom day/night transforms are omitted until environment attributes
//! drive `EyeblossomBlock`.

use std::sync::{Arc, LazyLock};

use rustc_hash::FxHashMap;
use steel_macros::block_behavior;
use steel_registry::REGISTRY;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_custom_stats::POT_FLOWER;
use steel_registry::{vanilla_blocks, vanilla_game_events};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, Direction, Identifier};

use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::behavior::{ITEM_BEHAVIORS, InventoryAccess};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{LevelReader, ScheduledTickAccess, World};

static POTTED_BY_CONTENT: LazyLock<SyncMutex<FxHashMap<Identifier, BlockRef>>> =
    LazyLock::new(|| SyncMutex::new(FxHashMap::default()));

/// Behavior for flower pots and every potted plant variant.
#[block_behavior]
pub struct FlowerPotBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks, json = "potted")]
    potted: BlockRef,
}

impl FlowerPotBlock {
    /// Creates flower-pot behavior and records the content → potted-block mapping.
    #[must_use]
    pub fn new(block: BlockRef, potted: BlockRef) -> Self {
        if potted != &vanilla_blocks::AIR {
            POTTED_BY_CONTENT.lock().insert(potted.key.clone(), block);
        }
        Self { block, potted }
    }

    fn is_empty(&self) -> bool {
        self.potted == &vanilla_blocks::AIR
    }

    fn potted_block_for(content: BlockRef) -> Option<BlockRef> {
        POTTED_BY_CONTENT.lock().get(&content.key).copied()
    }
}

impl BlockBehavior for FlowerPotBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    fn update_shape(
        &self,
        state: BlockStateId,
        world: &dyn ScheduledTickAccess,
        pos: BlockPos,
        direction: Direction,
        _neighbor_pos: BlockPos,
        _neighbor_state: BlockStateId,
    ) -> BlockStateId {
        // Vanilla `FlowerPotBlock.updateShape`: only the down neighbor can break the pot,
        // and only when `canSurvive` is false (default true; no override).
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
        }
    }

    fn use_item_on(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
        inv: &mut InventoryAccess,
    ) -> InteractionResult {
        let Some(content) =
            inv.with_item(|item| ITEM_BEHAVIORS.get_behavior(item.item()).placed_block())
        else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        let Some(potted_block) = Self::potted_block_for(content) else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        if !self.is_empty() {
            return InteractionResult::Consume;
        }

        world.set_block(pos, potted_block.default_state(), UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );
        player.award_custom_stat(&POT_FLOWER);
        if !player.has_infinite_materials() {
            inv.with_item(|item| item.shrink(1));
        }
        InteractionResult::Success
    }

    fn use_without_item(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        _hit_result: &BlockHitResult,
        _inv: &mut InventoryAccess,
    ) -> InteractionResult {
        if self.is_empty() {
            return InteractionResult::Consume;
        }

        player.add_item_or_drop(ItemStack::new(REGISTRY.items.by_block(self.potted)));
        world.set_block(
            pos,
            vanilla_blocks::FLOWER_POT.default_state(),
            UpdateFlags::UPDATE_ALL,
        );
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );
        InteractionResult::Success
    }

    fn get_clone_item_stack(
        &self,
        block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        if self.is_empty() {
            Some(ItemStack::new(REGISTRY.items.by_block(block)))
        } else {
            Some(ItemStack::new(REGISTRY.items.by_block(self.potted)))
        }
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::init_vanilla_registry;
    use steel_registry::vanilla_items;
    use steel_utils::ChunkPos;
    use steel_utils::types::InteractionHand;

    use crate::behavior::init_behaviors;
    use crate::entity::Entity;
    use crate::test_support::{TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk};

    use super::*;

    fn test_player(world: &Arc<World>) -> Arc<Player> {
        let player = TestPlayerBuilder::new(Arc::clone(world), "PotTester", 1).build();
        player
            .try_set_position(DVec3::new(0.5, 65.0, 0.5))
            .expect("test player should move onto the pot chunk");
        player
    }

    fn interact(
        behavior: &FlowerPotBlock,
        world: &Arc<World>,
        pos: BlockPos,
        player: &Player,
        with_item: bool,
    ) -> InteractionResult {
        let mut inv = InventoryAccess::new(player.inventory.clone(), InteractionHand::MainHand);
        let hit = BlockHitResult {
            location: DVec3::new(0.5, 65.5, 0.5),
            direction: Direction::Up,
            block_pos: pos,
            miss: false,
            inside: false,
            world_border_hit: false,
        };
        let state = world.get_block_state(pos);
        if with_item {
            behavior.use_item_on(
                state,
                world,
                pos,
                player,
                InteractionHand::MainHand,
                &hit,
                &mut inv,
            )
        } else {
            behavior.use_without_item(state, world, pos, player, &hit, &mut inv)
        }
    }

    #[test]
    fn potted_map_resolves_dandelion_to_potted_dandelion() {
        init_vanilla_registry();
        init_behaviors();

        assert_eq!(
            FlowerPotBlock::potted_block_for(&vanilla_blocks::DANDELION),
            Some(&vanilla_blocks::POTTED_DANDELION)
        );
        assert_eq!(
            FlowerPotBlock::potted_block_for(&vanilla_blocks::STONE),
            None
        );
    }

    #[test]
    fn empty_pot_accepts_a_dandelion_and_consumes_it() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("flower_pot_fill");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::FLOWER_POT.default_state(),
            UpdateFlags::UPDATE_NONE
        ));

        let player = test_player(&world);
        player.inventory.lock().set_item_in_hand(
            InteractionHand::MainHand,
            ItemStack::with_count(&vanilla_items::DANDELION, 2),
        );

        let empty = FlowerPotBlock::new(&vanilla_blocks::FLOWER_POT, &vanilla_blocks::AIR);
        assert_eq!(
            interact(&empty, &world, pos, &player, true),
            InteractionResult::Success
        );
        assert_eq!(
            world.get_block_state(pos).get_block(),
            &vanilla_blocks::POTTED_DANDELION
        );
        assert_eq!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .count(),
            1
        );
    }

    #[test]
    fn filled_pot_returns_the_plant_and_becomes_empty() {
        init_vanilla_registry();
        init_behaviors();

        let world = fresh_test_world("flower_pot_empty");
        let pos = BlockPos::new(0, 64, 0);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::POTTED_DANDELION.default_state(),
            UpdateFlags::UPDATE_NONE
        ));

        let player = test_player(&world);
        let filled = FlowerPotBlock::new(
            &vanilla_blocks::POTTED_DANDELION,
            &vanilla_blocks::DANDELION,
        );
        assert_eq!(
            interact(&filled, &world, pos, &player, false),
            InteractionResult::Success
        );
        assert_eq!(
            world.get_block_state(pos).get_block(),
            &vanilla_blocks::FLOWER_POT
        );
        assert!(
            player
                .inventory
                .lock()
                .get_item_in_hand(InteractionHand::MainHand)
                .is(&vanilla_items::DANDELION)
        );
    }

    #[test]
    fn clone_item_of_a_filled_pot_is_the_plant() {
        init_vanilla_registry();

        let empty = FlowerPotBlock::new(&vanilla_blocks::FLOWER_POT, &vanilla_blocks::AIR);
        assert_eq!(
            empty
                .get_clone_item_stack(
                    &vanilla_blocks::FLOWER_POT,
                    vanilla_blocks::FLOWER_POT.default_state(),
                    false
                )
                .map(|stack| stack.item().key.clone()),
            Some(vanilla_items::FLOWER_POT.key.clone())
        );

        let filled = FlowerPotBlock::new(
            &vanilla_blocks::POTTED_DANDELION,
            &vanilla_blocks::DANDELION,
        );
        assert_eq!(
            filled
                .get_clone_item_stack(
                    &vanilla_blocks::POTTED_DANDELION,
                    vanilla_blocks::POTTED_DANDELION.default_state(),
                    false
                )
                .map(|stack| stack.item().key.clone()),
            Some(vanilla_items::DANDELION.key.clone())
        );
    }
}
