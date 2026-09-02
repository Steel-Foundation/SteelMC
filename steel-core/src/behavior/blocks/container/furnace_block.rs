//! Normal furnace block behavior.

use std::sync::{Arc, Weak};

use glam::DVec3;
use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt as _;
use steel_registry::blocks::properties::{BlockStateProperties, Direction};
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, Downcast as _};

use crate::behavior::{
    BlockBehavior, BlockEntityCreation, BlockHitResult, BlockPlaceContext, InteractionResult,
    InventoryAccess,
};
use crate::block_entity::base_container::BaseContainer;
use crate::block_entity::entities::FurnaceBlockEntity;
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker, SharedBlockEntity};
use crate::inventory::container::calculate_redstone_signal_from_container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::furnace as furnace_menu;
use crate::inventory::menu::{MenuCreation, MenuProvider};
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Vanilla normal-furnace block behavior.
#[block_behavior]
pub struct FurnaceBlock {
    block: BlockRef,
}

impl FurnaceBlock {
    /// Creates normal-furnace behavior for `block`.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

/// Mirrors `FurnaceBlockEntity` acting as its own Vanilla `MenuProvider`.
struct FurnaceMenuProvider {
    world: Arc<World>,
    pos: BlockPos,
    block_entity: SharedBlockEntity,
}

impl MenuProvider for FurnaceMenuProvider {
    fn create_menu(self: Box<Self>, player: &Player) -> MenuCreation {
        let Some(furnace) = self.block_entity.downcast_ref::<FurnaceBlockEntity>() else {
            return MenuCreation::Unavailable;
        };
        let title = furnace.display_name();
        if !furnace.can_open(player) {
            BaseContainer::send_chest_locked_notifications(
                &self.world,
                DVec3::new(
                    f64::from(self.pos.x()) + 0.5,
                    f64::from(self.pos.y()) + 0.5,
                    f64::from(self.pos.z()) + 0.5,
                ),
                player,
                title,
            );
            return MenuCreation::Unavailable;
        }
        let Some(container) = self.block_entity.container_ref() else {
            return MenuCreation::Unavailable;
        };
        let inventory = player.inventory.clone();
        player.open_menu(title, move |context| {
            furnace_menu(inventory, context.container_id, container)
        });
        MenuCreation::Opened
    }
}

impl BlockBehavior for FurnaceBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        Some(self.block.default_state().set_value(
            &BlockStateProperties::HORIZONTAL_FACING,
            context.horizontal_direction().opposite(),
        ))
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
        if let Some(provider) = self.get_menu_provider(state, world, pos) {
            player.open_menu_provider(provider);
            // TODO: Award INTERACT_WITH_FURNACE once Steel has statistics.
        }
        InteractionResult::Success
    }

    fn get_menu_provider(
        &self,
        _state: BlockStateId,
        world: &Arc<World>,
        pos: BlockPos,
    ) -> Option<Box<dyn MenuProvider>> {
        let block_entity = world.get_block_entity(pos)?;
        block_entity.downcast_ref::<FurnaceBlockEntity>()?;
        Some(Box::new(FurnaceMenuProvider {
            world: Arc::clone(world),
            pos,
            block_entity,
        }))
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::FURNACE,
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
            &vanilla_block_entity_types::FURNACE,
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
        world: &dyn LevelReader,
        pos: BlockPos,
        _direction: Direction,
    ) -> i32 {
        let Some(block_entity) = world.get_block_entity(pos) else {
            return 0;
        };
        let Some(container) = ContainerRef::from_block_entity(block_entity) else {
            return 0;
        };
        let guard = ContainerLockGuard::lock_all(&[&container]);
        guard
            .get(container.container_id())
            .map_or(0, calculate_redstone_signal_from_container)
    }

    // Vanilla's crackle sound, smoke, and flame are client-local animateTick
    // effects. Steel only needs to synchronize the LIT block state here.
}

#[cfg(test)]
mod tests {
    use steel_registry::item_stack::ItemStack;
    use steel_registry::test_support::init_test_registry;
    use steel_registry::{vanilla_blocks, vanilla_items};
    use steel_utils::ChunkPos;
    use steel_utils::types::UpdateFlags;

    use super::*;
    use crate::behavior::init_behaviors;
    use crate::block_entity::init_block_entities;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    #[test]
    fn ticker_is_selected_only_for_the_normal_furnace_entity() {
        init_test_registry();
        let world = fresh_test_world("furnace_ticker_selection");
        let behavior = FurnaceBlock::new(&vanilla_blocks::FURNACE);
        let state = vanilla_blocks::FURNACE.default_state();

        assert!(
            behavior
                .get_block_entity_ticker(&world, state, &vanilla_block_entity_types::FURNACE)
                .is_some()
        );
        assert!(
            behavior
                .get_block_entity_ticker(&world, state, &vanilla_block_entity_types::CHEST)
                .is_none()
        );
    }

    #[test]
    fn first_server_tick_ignites_and_synchronizes_the_lit_state() {
        init_test_registry();
        init_behaviors();
        init_block_entities();
        let world = fresh_test_world("furnace_first_server_tick");
        let pos = BlockPos::new(2, 64, 2);
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(pos));
        assert!(world.set_block(
            pos,
            vanilla_blocks::FURNACE.default_state(),
            UpdateFlags::UPDATE_NONE,
        ));
        let Some(block_entity) = world.get_block_entity(pos) else {
            panic!("furnace placement should create its block entity");
        };
        assert!(block_entity.downcast_ref::<FurnaceBlockEntity>().is_some());
        let Some(container) = block_entity.container_ref() else {
            panic!("furnace should expose its inventory");
        };
        let container_id = container.container_id();
        let mut guard = ContainerLockGuard::lock_all(&[container]);
        assert!(guard.set_item(container_id, 0, ItemStack::new(&vanilla_items::RAW_IRON),));
        assert!(guard.set_item(container_id, 1, ItemStack::new(&vanilla_items::COAL),));
        drop(guard);

        world.block_entity_tickers().tick(&world, true);

        assert!(
            world
                .get_block_state(pos)
                .get_value(&BlockStateProperties::LIT)
        );
        let Some(container) = block_entity.container_ref() else {
            panic!("furnace should retain its inventory");
        };
        let guard = ContainerLockGuard::lock_all(&[container]);
        let Some(container) = guard.get(container_id) else {
            panic!("furnace inventory should remain lockable");
        };
        assert!(container.get_item(0).is(&vanilla_items::RAW_IRON));
        assert!(container.get_item(1).is_empty());
    }
}
