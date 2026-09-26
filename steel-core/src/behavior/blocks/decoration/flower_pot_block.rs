//! `FlowerPotBlock` behavior (`net.minecraft.world.level.block.FlowerPotBlock`).
//!
//! TODO: Potted open/closed eyeblossoms do not transform at day/night yet.
//! Vanilla reads `EnvironmentAttributes.EYEBLOSSOM_OPEN`, which Steel lacks
//! (the same gap documented on `EyeblossomBlock`).

use std::sync::{Arc, LazyLock};

use rustc_hash::FxHashMap;
use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::items::item::BlockHitResult;
use steel_registry::vanilla_custom_stats;
use steel_registry::{REGISTRY, RegistryExt as _, vanilla_blocks, vanilla_game_events};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, Direction, Identifier};

use crate::behavior::{BlockBehavior, BlockPlaceContext, InteractionResult, InventoryAccess};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::game_event::GameEventContext;
use crate::world::{ScheduledTickAccess, World};

/// Maps a potted content block to the flower pot block that holds it.
///
/// Populated by [`FlowerPotBlock::new`], mirroring vanilla
/// `FlowerPotBlock.POTTED_BY_CONTENT`.
static POTTED_BY_CONTENT: LazyLock<SyncMutex<FxHashMap<Identifier, BlockRef>>> =
    LazyLock::new(|| SyncMutex::new(FxHashMap::default()));

/// Vanilla `FlowerPotBlock`.
#[block_behavior]
pub struct FlowerPotBlock {
    block: BlockRef,
    /// The plant block inside the pot, or `air` for an empty pot.
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

    /// Vanilla `FlowerPotBlock.isEmpty`.
    fn is_empty(&self) -> bool {
        self.potted == &vanilla_blocks::AIR
    }

    /// Returns the potted block holding `content`, mirroring vanilla
    /// `FlowerPotBlock.POTTED_BY_CONTENT`.
    fn potted_block_for(content: BlockRef) -> Option<BlockRef> {
        POTTED_BY_CONTENT.lock().get(&content.key).copied()
    }
}

impl BlockBehavior for FlowerPotBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
        // Block items share their block's identifier, so the held item's key
        // resolves the content block without a reverse item lookup.
        //TODO: item --> block mapper should be fixed
        let content = inv.with_item(|item| REGISTRY.blocks.by_key(&item.item().key));
        let Some(new_contents) = content.and_then(Self::potted_block_for) else {
            return InteractionResult::TryEmptyHandInteraction;
        };

        if !self.is_empty() {
            return InteractionResult::Consume;
        }

        world.set_block(pos, new_contents.default_state(), UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );
        player.award_custom_stat(&vanilla_custom_stats::POT_FLOWER);
        let has_infinite_materials = player.has_infinite_materials();
        inv.with_item(|item| item.consume_one(has_infinite_materials));
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
        let item = if self.is_empty() {
            REGISTRY.items.by_block(block)
        } else {
            REGISTRY.items.by_block(self.potted)
        };
        Some(ItemStack::new(item))
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
        // Vanilla checks `state.canSurvive`, but `FlowerPotBlock` inherits the
        // default `true`, so this never breaks a floating pot.
        if direction == Direction::Down && !self.can_survive(state, world, pos) {
            vanilla_blocks::AIR.default_state()
        } else {
            state
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
    use steel_registry::blocks::block_state_ext::BlockStateExt as _;
    use steel_registry::{init_vanilla_registry, vanilla_items};
    use steel_utils::{ChunkPos, Direction};

    use super::*;
    use crate::bootstrap::init_globals;
    use crate::test_support::{
        TestLevel, TestPlayerBuilder, fresh_test_world, insert_ready_full_chunk,
    };

    const TEST_POS: BlockPos = BlockPos::new(8, 64, 8);
    const TEST_PLAYER_ENTITY_ID: i32 = 1;

    fn empty_pot() -> FlowerPotBlock {
        FlowerPotBlock::new(&vanilla_blocks::FLOWER_POT, &vanilla_blocks::AIR)
    }

    fn potted_dandelion() -> FlowerPotBlock {
        FlowerPotBlock::new(
            &vanilla_blocks::POTTED_DANDELION,
            &vanilla_blocks::DANDELION,
        )
    }

    fn interaction_hit() -> BlockHitResult {
        BlockHitResult {
            location: DVec3::new(8.5, 65.0, 8.5),
            direction: Direction::Up,
            block_pos: TEST_POS,
            miss: false,
            inside: false,
            world_border_hit: false,
        }
    }

    fn test_player(world: &Arc<World>) -> Arc<Player> {
        TestPlayerBuilder::new(Arc::clone(world), "FlowerPotTester", TEST_PLAYER_ENTITY_ID).build()
    }

    #[test]
    fn placing_and_removing_a_plant_swaps_the_potted_state() {
        init_globals();
        let world = fresh_test_world("flower_pot_interaction");
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(TEST_POS));
        let player = test_player(&world);
        let mut inventory =
            InventoryAccess::new(Arc::clone(&player.inventory), InteractionHand::MainHand);

        assert!(world.set_block(
            TEST_POS,
            vanilla_blocks::FLOWER_POT.default_state(),
            UpdateFlags::UPDATE_ALL
        ));

        // Holding a plant fills the empty pot and consumes the item.
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::with_count(&vanilla_items::DANDELION, 1));
        assert_eq!(
            empty_pot().use_item_on(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                &player,
                InteractionHand::MainHand,
                &interaction_hit(),
                &mut inventory,
            ),
            InteractionResult::Success,
        );
        assert_eq!(
            world.get_block_state(TEST_POS).get_block(),
            &vanilla_blocks::POTTED_DANDELION
        );
        assert!(player.inventory.lock().get_selected_item().is_empty());

        // A non-plant item falls through to the empty-hand interaction, which
        // removes the plant and returns it to the inventory.
        player
            .inventory
            .lock()
            .set_selected_item(ItemStack::new(&vanilla_items::STONE));
        assert_eq!(
            potted_dandelion().use_item_on(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                &player,
                InteractionHand::MainHand,
                &interaction_hit(),
                &mut inventory,
            ),
            InteractionResult::TryEmptyHandInteraction,
        );
        assert_eq!(
            potted_dandelion().use_without_item(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                &player,
                &interaction_hit(),
                &mut inventory,
            ),
            InteractionResult::Success,
        );
        assert_eq!(
            world.get_block_state(TEST_POS).get_block(),
            &vanilla_blocks::FLOWER_POT
        );
        assert!(
            player
                .inventory
                .lock()
                .get_items()
                .iter()
                .any(|item| item.is(&vanilla_items::DANDELION))
        );

        // An empty pot has nothing to remove.
        assert_eq!(
            empty_pot().use_without_item(
                world.get_block_state(TEST_POS),
                &world,
                TEST_POS,
                &player,
                &interaction_hit(),
                &mut inventory,
            ),
            InteractionResult::Consume,
        );
    }

    #[test]
    fn update_shape_leaves_a_floating_pot_intact() {
        init_vanilla_registry();
        let behavior = empty_pot();
        let state = vanilla_blocks::FLOWER_POT.default_state();
        // No block below: Java flower pots float, so the DOWN update must not
        // break the pot.
        let level = TestLevel::default();
        assert_eq!(
            behavior.update_shape(
                state,
                &level,
                BlockPos::ZERO,
                Direction::Down,
                BlockPos::ZERO.below(),
                vanilla_blocks::AIR.default_state(),
            ),
            state,
        );
    }

    #[test]
    fn clone_item_stack_returns_the_potted_plant() {
        init_vanilla_registry();
        let empty_clone = empty_pot()
            .get_clone_item_stack(
                &vanilla_blocks::FLOWER_POT,
                vanilla_blocks::FLOWER_POT.default_state(),
                false,
            )
            .expect("flower pot always yields a clone stack");
        assert!(empty_clone.is(&vanilla_items::FLOWER_POT));

        let filled_clone = potted_dandelion()
            .get_clone_item_stack(
                &vanilla_blocks::POTTED_DANDELION,
                vanilla_blocks::POTTED_DANDELION.default_state(),
                false,
            )
            .expect("potted flower pot always yields a clone stack");
        assert!(filled_clone.is(&vanilla_items::DANDELION));
    }
}
