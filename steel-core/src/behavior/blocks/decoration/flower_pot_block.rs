use rustc_hash::FxHashMap;
use std::sync::{Arc, OnceLock};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_game_events;
use steel_registry::{REGISTRY, RegistryExt, vanilla_blocks};
use steel_utils::locks::SyncMutex;
use steel_utils::types::{InteractionHand, UpdateFlags};
use steel_utils::{BlockPos, BlockStateId, Identifier};

use crate::behavior::{
    BlockBehavior, BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess,
};
use crate::entity::ai::path::PathComputationType;
use crate::player::Player;
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Content item key to potted block variant, the equivalent of vanilla's
/// `FlowerPotBlock.POTTED_BY_CONTENT`. Vanilla fills this map from every
/// flower-pot constructor call; Steel fills it from every generated
/// behavior registration.
pub(crate) static POTTED_BY_CONTENT: OnceLock<SyncMutex<FxHashMap<Identifier, BlockRef>>> =
    OnceLock::new();

#[block_behavior]
pub struct FlowerPotBlock {
    block: BlockRef,
    #[json_arg(vanilla_blocks)]
    potted: BlockRef,
}

impl FlowerPotBlock {
    /// Creates the behavior for one registered pot block.
    pub fn new(block: BlockRef, potted: BlockRef) -> Self {
        POTTED_BY_CONTENT
            .get_or_init(|| SyncMutex::new(FxHashMap::default()))
            .lock()
            .insert(REGISTRY.items.by_block(potted).key.clone(), block);
        Self { block, potted }
    }

    fn is_empty(&self) -> bool {
        self.potted == &vanilla_blocks::AIR
    }
}

impl BlockBehavior for FlowerPotBlock {
    /// Returns the empty pot's default state when placed.
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
    }

    /// Vanilla `FlowerPotBlock.useItemOn`.
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
        let held_potted = inv.with_item(|stack| {
            if stack.is_empty() {
                None
            } else {
                POTTED_BY_CONTENT
                    .get_or_init(|| SyncMutex::new(FxHashMap::default()))
                    .lock()
                    .get(&stack.item().key)
                    .copied()
            }
        });
        let Some(potted_variant) = held_potted else {
            return InteractionResult::TryEmptyHandInteraction;
        };
        if !self.is_empty() {
            return InteractionResult::Consume;
        }

        world.set_block(pos, potted_variant.default_state(), UpdateFlags::UPDATE_ALL);
        world.game_event(
            &vanilla_game_events::BLOCK_CHANGE,
            pos,
            &GameEventContext::new(Some(player), None),
        );
        inv.with_item(|stack| stack.shrink(1));
        // TODO: Award the POT_FLOWER statistic once stats exist.
        InteractionResult::Success
    }

    /// Vanilla `FlowerPotBlock.useWithoutItem`.
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

        let plant = ItemStack::new(REGISTRY.items.by_block(self.potted));
        player.add_item_or_drop(plant);
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

    /// Vanilla returns the content item for potted pots and falls back to the
    /// default (block-key lookup) for the empty pot.
    fn get_clone_item_stack(
        &self,
        block: BlockRef,
        _state: BlockStateId,
        _include_data: bool,
    ) -> Option<ItemStack> {
        if self.is_empty() {
            REGISTRY.items.by_key(&block.key).map(ItemStack::new)
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

    // No update_shape/can_survive overrides: vanilla only breaks the pot when its
    // DOWN neighbor invalidates survival, but FlowerPotBlock never overrides
    // canSurvive (default true), so pots float when support is removed. The
    // observable behavior matches the trait defaults.

    // The potted open/closed eyeblossom random-tick day/night transform needs the
    // EnvironmentAttributes foundation and is not implemented yet.
}
