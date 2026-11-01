//! Barrel block behavior implementation.
//!
//! Opens a 27-slot container menu when right-clicked.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::{vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::BLOCK_ENTITIES;
use crate::entity::Entity as _;
use crate::inventory::container::calculate_redstone_signal_from_containers;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::chest;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Behavior for barrel blocks.
///
/// Barrels are container block entities with 27 slots (3x9 grid).
/// They use the same menu as chests but cannot form double containers.
#[block_behavior]
pub struct BarrelBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;

impl BarrelBlock {
    /// Creates a new barrel block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BarrelBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        // Barrel faces opposite to the player's look direction (all 6 directions).
        let facing = context.get_nearest_looking_direction().opposite();

        Some(self.block.default_state().set_value(FACING, facing))
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
        // Get the block entity
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        if let Some(randomizable) = block_entity.as_randomizable_container() {
            if player.is_spectator() && randomizable.loot_table().is_some() {
                return InteractionResult::Success;
            }
            if let Err(error) = randomizable.unpack_loot_table(Some(player)) {
                log::error!("Could not unpack barrel loot at {pos:?}: {error}");
                return InteractionResult::Success;
            }
        }
        let Some(container_ref) = block_entity.container_ref() else {
            return InteractionResult::Pass;
        };

        // Open the chest menu (3 rows for barrel)
        let inventory = Arc::clone(&player.inventory);
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_BARREL.msg()),
            move |context| chest(inventory, context.container_id, container_ref, 3),
        );

        player.award_custom_stat(&vanilla_custom_stats::OPEN_BARREL);
        // TODO: Anger nearby piglins (PiglinAi.angerNearbyPiglins)
        // TODO: Implement ContainerOpenersCounter to track open state, play sounds,
        //       and update OPEN block property. Requires scheduled block ticks (scheduleTick)
        //       for recheck functionality. See vanilla BarrelBlockEntity and ContainerOpenersCounter.

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BARREL,
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
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        // Get the block entity and calculate signal from container contents
        let Some(container_ref) = world
            .get_block_entity(pos)
            .and_then(ContainerRef::from_block_entity)
        else {
            return 0;
        };
        let Ok(guard) = ContainerLockGuard::lock_all(&[&container_ref]) else {
            // TODO: Preserve vanilla access ordering so loot-generation contention
            // cannot become a false zero comparator signal. Returning zero here
            // also hides generation failures; distinguish them from empty contents.
            return 0;
        };
        calculate_redstone_signal_from_containers(guard.get(container_ref.container_id()))
    }
}
