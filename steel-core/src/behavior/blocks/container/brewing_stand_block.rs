//! Brewing-stand block behavior.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::properties::Direction;
use steel_registry::{sound_events, vanilla_block_entity_types, vanilla_custom_stats};
use steel_utils::{BlockPos, BlockStateId, Downcast as _, translations};

use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::{BlockHitResult, BlockPlaceContext, InteractionResult, InventoryAccess};
use crate::block_entity::entities::BrewingStandBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::entity::Entity as _;
use crate::entity::ai::path::PathComputationType;
use crate::inventory::container::calculate_redstone_signal_from_containers;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::brewing_stand_menu;
use crate::nameable::Nameable as _;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Behavior for brewing stand blocks.
#[block_behavior]
pub struct BrewingStandBlock {
    block: BlockRef,
}

impl BrewingStandBlock {
    /// Creates this behavior for its registered block.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

fn analog_output(world: &(impl LevelReader + ?Sized), pos: BlockPos) -> i32 {
    let Some(container) = world
        .get_block_entity(pos)
        .and_then(ContainerRef::from_block_entity)
    else {
        return 0;
    };
    let Ok(guard) = ContainerLockGuard::lock_all(&[&container]) else {
        // TODO: Handle preparation failures without reporting an empty inventory.
        // Brewing-stand storage has no preparation callback; ordinary mutex
        // contention waits instead of reaching this fallback.
        return 0;
    };
    calculate_redstone_signal_from_containers(guard.get(container.container_id()))
}

impl BlockBehavior for BrewingStandBlock {
    fn get_state_for_placement(&self, _context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state())
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
            return InteractionResult::Success;
        };
        let Some(brewing_stand) = block_entity.downcast_ref::<BrewingStandBlockEntity>() else {
            return InteractionResult::Success;
        };
        let title = brewing_stand.name();
        player.award_custom_stat(&vanilla_custom_stats::INTERACT_WITH_BREWINGSTAND);
        if !brewing_stand.can_open(player) {
            if !player.is_spectator() {
                player.send_overlay_message(
                    &translations::CONTAINER_IS_LOCKED
                        .message([title])
                        .component(),
                );
                world.play_sound_at(
                    &sound_events::BLOCK_CHEST_LOCKED,
                    SoundSource::Blocks,
                    pos.get_center(),
                    1.0,
                    1.0,
                    None,
                );
            }
            return InteractionResult::Success;
        }
        let Some(container) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Success;
        };

        let inventory = Arc::clone(&player.inventory);
        player.open_menu(title, move |context| {
            brewing_stand_menu(inventory, context.container_id, container)
        });
        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BREWING_STAND,
            level,
            pos,
            state,
        ))
    }

    fn get_block_entity_ticker(
        &self,
        _world: &Arc<World>,
        _state: BlockStateId,
        block_entity_type: BlockEntityTypeRef,
    ) -> Option<BlockEntityTicker> {
        BlockEntityTicker::for_matching_entity_tick(
            block_entity_type,
            &vanilla_block_entity_types::BREWING_STAND,
        )
    }

    fn affect_neighbors_after_removal(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
        _moved_by_piston: bool,
    ) {
        world.update_neighbor_for_output_signal(pos, self.block);
    }

    fn has_analog_output_signal(&self, _state: BlockStateId) -> bool {
        true
    }

    fn get_analog_output_signal(
        &self,
        _state: BlockStateId,
        world: &World,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        analog_output(world, pos)
    }

    fn is_pathfindable(
        &self,
        _state: BlockStateId,
        _computation_type: PathComputationType,
    ) -> bool {
        false
    }
}
