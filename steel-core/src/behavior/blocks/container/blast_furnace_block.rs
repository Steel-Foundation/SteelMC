//! Blast furnace block behavior implementation.
//!
//! Opens the blast furnace menu when interacted with and manages the LIT block state.

use std::sync::{Arc, Weak};

use steel_macros::block_behavior;
use steel_registry::block_entity_type::BlockEntityTypeRef;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::blocks::properties::{BlockStateProperties, Direction, EnumProperty};
use steel_registry::vanilla_block_entity_types;
use steel_utils::{BlockPos, BlockStateId, translations};
use text_components::TextComponent;

use crate::behavior::InventoryAccess;
use crate::behavior::block::{BlockBehavior, BlockEntityCreation};
use crate::behavior::context::{BlockHitResult, BlockPlaceContext, InteractionResult};
use crate::block_entity::{BLOCK_ENTITIES, BlockEntityTicker};
use crate::inventory::container::calculate_redstone_signal_from_container;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::menu::kinds::blast_furnace;
use crate::player::Player;
use crate::world::{LevelReader, World};

/// Blast furnace block behavior.
#[block_behavior]
pub struct BlastFurnaceBlock {
    block: BlockRef,
}

const FACING: &EnumProperty<Direction> = &BlockStateProperties::FACING;
const LIT: steel_registry::blocks::properties::BoolProperty = BlockStateProperties::LIT;

impl BlastFurnaceBlock {
    /// Creates a new blast furnace block behavior.
    #[must_use]
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl BlockBehavior for BlastFurnaceBlock {
    fn get_state_for_placement(&self, context: &BlockPlaceContext<'_>) -> Option<BlockStateId> {
        let facing = context.get_nearest_looking_direction().opposite();
        Some(
            self.block
                .default_state()
                .set_value(FACING, facing)
                .set_value(&LIT, false),
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
        let Some(block_entity) = world.get_block_entity(pos) else {
            return InteractionResult::Pass;
        };

        let Some(container_ref) = ContainerRef::from_block_entity(block_entity) else {
            return InteractionResult::Pass;
        };

        let inventory = player.inventory.clone();
        player.open_menu(
            TextComponent::translated(translations::CONTAINER_BLAST_FURNACE.msg()),
            move |context| blast_furnace(inventory, context.container_id, container_ref),
        );

        // TODO: Award stat INTERACT_WITH_BLAST_FURNACE

        InteractionResult::Success
    }

    fn new_block_entity(
        &self,
        level: Weak<World>,
        pos: BlockPos,
        state: BlockStateId,
    ) -> BlockEntityCreation {
        BlockEntityCreation::from_registered_factory(BLOCK_ENTITIES.create(
            &vanilla_block_entity_types::BLAST_FURNACE,
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
            &vanilla_block_entity_types::BLAST_FURNACE,
        )
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
                calculate_redstone_signal_from_container(container)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_block_entity_types};
    use crate::test_support::fresh_test_world;
    use crate::block_entity::init_block_entities;

    #[test]
    fn blast_furnace_has_analog_output() {
        init_vanilla_registry();
        let block = BlastFurnaceBlock::new(&vanilla_blocks::BLAST_FURNACE);
        assert!(block.has_analog_output_signal(vanilla_blocks::BLAST_FURNACE.default_state()));
    }

    #[test]
    fn blast_furnace_new_block_entity_and_ticker() {
        init_vanilla_registry();
        init_block_entities();
        let world = fresh_test_world("bf_entity_test");
        let block = BlastFurnaceBlock::new(&vanilla_blocks::BLAST_FURNACE);
        let state = vanilla_blocks::BLAST_FURNACE.default_state();
        let pos = BlockPos::new(0, 64, 0);

        let created = block.new_block_entity(Arc::downgrade(&world), pos, state);
        assert!(created.into_created().is_some());

        let ticker = block.get_block_entity_ticker(&world, state, &vanilla_block_entity_types::BLAST_FURNACE);
        assert!(ticker.is_some());

        let wrong_ticker = block.get_block_entity_ticker(&world, state, &vanilla_block_entity_types::BARREL);
        assert!(wrong_ticker.is_none());
    }

    #[test]
    fn blast_furnace_analog_output_signal() {
        init_vanilla_registry();
        init_block_entities();
        let world = fresh_test_world("bf_signal_test");
        let block = BlastFurnaceBlock::new(&vanilla_blocks::BLAST_FURNACE);
        let state = vanilla_blocks::BLAST_FURNACE.default_state();
        let pos = BlockPos::new(10, 64, 10);

        // Signal with no block entity at pos is 0
        let signal_empty = block.get_analog_output_signal(state, &*world, pos, Direction::North);
        assert_eq!(signal_empty, 0);
    }
}
