use std::sync::Arc;
use steel_registry::blocks::BlockRef;
use steel_registry::blocks::block_state_ext::BlockStateExt;
use steel_registry::item_stack::ItemStack;
use steel_registry::level_events;
use steel_utils::BlockPos;
use steel_utils::BlockStateId;
use steel_utils::Direction;

use super::DispenseItemBehavior;
use crate::behavior::blocks::container::dispenser_block::FACING;
use crate::behavior::context::BlockPlaceContext;
use crate::behavior::items::BlockItem;
use crate::world::World;

pub struct ShulkerBoxDispenseBehavior {
    block: BlockRef,
}

impl ShulkerBoxDispenseBehavior {
    pub const fn new(block: BlockRef) -> Self {
        Self { block }
    }
}

impl DispenseItemBehavior for ShulkerBoxDispenseBehavior {
    fn dispense(
        &self,
        world: &Arc<World>,
        pos: BlockPos,
        state: BlockStateId,
        mut item: ItemStack,
    ) -> ItemStack {
        let facing = state.get_value(FACING);
        let target_pos = pos.relative(facing);
        let clicked_face = if world.get_block_state(target_pos.below()).is_air() {
            facing
        } else {
            Direction::Up
        };

        let context =
            BlockPlaceContext::directional(world, target_pos, facing, &mut item, clicked_face);
        let placed = BlockItem::new(self.block).place(context).consumes_action();

        if !placed {
            world.level_event(level_events::SOUND_DISPENSER_FAIL, pos, 0, None);
        }

        item
    }
}

// TODO: chest — needs ChestBlock, which doesn't exist yet.

#[cfg(test)]
mod tests {
    use steel_registry::{init_vanilla_registry, vanilla_blocks, vanilla_items};
    use steel_utils::{ChunkPos, types::UpdateFlags};

    use crate::behavior::blocks::container::dispenser_block::DispenserBlock;
    use crate::behavior::init_behaviors;
    use crate::block_entity::entities::DispenserBlockEntity;
    use crate::block_entity::init_block_entities;
    use crate::inventory::container::Container;
    use crate::test_support::{fresh_test_world, insert_ready_full_chunk};

    use super::*;

    #[test]
    fn dispenser_places_shulker_box() {
        init_vanilla_registry();
        init_block_entities();
        init_behaviors();

        let world = fresh_test_world("dispenser_places_shulker_box");
        let dispenser_pos = BlockPos::new(8, 64, 8);
        let target_pos = dispenser_pos.relative(Direction::North);
        let _holder = insert_ready_full_chunk(&world, ChunkPos::from_block_pos(dispenser_pos));

        let state = vanilla_blocks::DISPENSER
            .default_state()
            .set_value(FACING, Direction::North);
        let entity = Arc::new(DispenserBlockEntity::new(
            Arc::downgrade(&world),
            dispenser_pos,
            state,
        ));
        entity
            .state
            .container()
            .lock()
            .set_item(0, ItemStack::with_count(&vanilla_items::SHULKER_BOX, 1));
        world.set_block(dispenser_pos, state, UpdateFlags::UPDATE_ALL);
        world.set_block_entity(entity.clone());

        DispenserBlock::dispense_from(&world, dispenser_pos, state);

        assert_eq!(entity.state.container().lock().get_item(0).count(), 0);
        assert_eq!(
            world.get_block_state(target_pos).get_block(),
            &vanilla_blocks::SHULKER_BOX
        );
    }
}
