//! Chest block behavior implementation.
//!
//! Opens a 27-slot container menu when right-clicked, rolling any pending
//! structure loot table first (vanilla `ChestBlockEntity.unpackLootTable`).

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::vanilla_block_entity_types;
use steel_utils::translations;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_utils::{BlockPos, BlockStateId, Downcast as _};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::block_entity::entities::ChestBlockEntity;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::chest;
use crate::player::Player;
use crate::world::World;

/// Behavior for chest blocks.
///
/// Steel difference: chests do not yet merge into double chests; interacting
/// with one half of a pair opens a single 27-slot menu.
#[block_behavior]
pub struct ChestBlock {
    block: BlockRef,
}

impl ChestBlock {
    /// Creates a new chest block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for ChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Chests face the player horizontally (vanilla `ChestBlock.getStateForPlacement`).
        let facing = context.get_nearest_looking_direction().opposite();
        let facing = if facing == Direction::Up || facing == Direction::Down {
            Direction::North
        } else {
            facing
        };
        Some(self.block.default_state().set_value(&BlockStateProperties::HORIZONTAL_FACING, facing))
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
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };
        // Vanilla rolls a structure loot table on first open.
        if let Some(chest) = block_entity.downcast_ref::<ChestBlockEntity>() {
            chest.try_populate_loot(world);
        }
        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Pass;
        };

        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_CHEST.msg()),
            move |context| chest(inventory, context.container_id, container_ref, 3),
        );

        // TODO: Implement ContainerOpenersCounter for lid open state, sounds, and
        //       the OPEN block property (same as barrel).

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::CHEST,
            level,
            pos,
            state,
        ))
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &dyn crate::world::LevelReader,
        pos: BlockPos,
        _direction: steel_registry::blocks::properties::Direction,
    ) -> i32 {
        let Some(container_ref) = world
            .get_block_entity(pos)
            .and_then(ContainerRef::from_block_entity)
        else {
            return 0;
        };
        let guard = ContainerLockGuard::lock_all(&[&container_ref]);
        guard
            .get(container_ref.container_id())
            .map_or(0, |container| {
                crate::inventory::container::calculate_redstone_signal_from_container(container)
            })
    }
}
