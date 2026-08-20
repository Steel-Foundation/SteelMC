//! Enderchest block behavior implementation.
//!
//! Opens a 27-slot container menu when right-clicked.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::inventory::menu::kinds::chest;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Behavior for ender chest blocks.
///
/// Ender Chests are block entities with 27 slots (3x9 grid) that store their contents on the player.
/// They use the same menu as chests but cannot form double containers.
#[block_behavior]
pub struct EnderChestBlock {
    block: BlockRef,
}

const HORIZONTAL_FACING: &EnumProperty<Direction> = &BlockStateProperties::HORIZONTAL_FACING;

impl EnderChestBlock {
    /// Creates a new ender chest block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for EnderChestBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(
            self.block
                .default_state()
                .set_value(HORIZONTAL_FACING, context.horizontal_direction().opposite()),
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
        let inventory = player.inventory.clone();
        let ender_chest = player.ender_chest.clone();

        // Ensure block entity exists
        let Some(_) = world.get_block_entity(pos) else {
            return InteractionResult::Success;
        };
        // Ensure ender chest can be opened
        let above = pos.offset(0, 1, 0);
        if world.get_block_state(above).is_static_redstone_conductor() {
            return InteractionResult::Success;
        }

        // Open the chest menu (3 rows for ender chest)
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_BARREL.msg()),
            move |context| chest(inventory, context.container_id, ender_chest, 3),
        );

        // TODO: Award stat OPEN_ENDERCHEST
        // TODO: Anger neaby piglins (PiglinAi.angerNearbyPiglins)
        // TODO: Implement open and close animations

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::ENDER_CHEST,
            level,
            pos,
            state,
        ))
    }
}
