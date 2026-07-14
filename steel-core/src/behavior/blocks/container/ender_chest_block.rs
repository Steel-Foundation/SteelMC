//! Ender chest block behavior implementation.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::BlockStateProperties;
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::BlockStateBehaviorExt;
use crate::behavior::InventoryAccess;
use crate::behavior::block::BlockBehavior;
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::{BLOCK_ENTITIES, SharedBlockEntity};
use crate::inventory::chest_menu::ChestMenuProvider;
use crate::player::Player;
use crate::world::World;

/// The ender chest block behavior.
#[block_behavior]
pub struct EnderChestBlock {
    block: BlockRef,
}

impl EnderChestBlock {
    /// Creates a new ender chest block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for EnderChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.horizontal_direction.opposite();

        let waterlogged = context.world.get_block_state(context.place_pos).has_fluid();

        Some(
            self.block
                .default_state()
                .set_value(&BlockStateProperties::FACING, facing)
                .set_value(&BlockStateProperties::WATERLOGGED, waterlogged),
        )
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
        // If there's a solid block above the chest, it cannot be opened
        let above_pos = pos.above();
        if world.get_block_state(above_pos).is_solid_render() {
            return InteractionResult::Pass;
        }

        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        // Ensure it's an ender chest block entity
        if block_entity.lock().get_type() != &vanilla_block_entity_types::ENDER_CHEST {
            return InteractionResult::Pass;
        }

        // Set the active block entity for the player's ender chest
        let mut ender_chest_inventory = player.ender_chest_inventory.lock();
        ender_chest_inventory.set_active_chest(Arc::downgrade(&block_entity));
        drop(ender_chest_inventory);

        // Open the menu using the player's ender chest container
        let container_ref = player.ender_chest_inventory.clone().into();
        player.open_menu(&ChestMenuProvider::three_rows(
            player.inventory.clone(),
            container_ref,
            TextComponent::translated(translations::CONTAINER_ENDERCHEST.msg()),
        ));

        // TODO: Award stat OPEN_ENDERCHEST
        // TODO: Anger nearby piglins

        InteractionResult::Success
    }

    fn has_block_entity(&self) -> bool {
        true
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> Option<SharedBlockEntity> {
        BLOCK_ENTITIES.create(&vanilla_block_entity_types::ENDER_CHEST, level, pos, state)
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        false
    }
}
