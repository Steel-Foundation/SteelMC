use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use steel_registry::{
    REGISTRY, TaggedRegistryExt, blocks::block_state_ext::BlockStateExt, item_stack::ItemStack,
    level_events, vanilla_block_tags::BlockTag,
};
use steel_utils::{BlockPos, BlockStateId, locks::Shared, types::UpdateFlags};

use crate::{
    behavior::blocks::AnvilBlock,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        lock::{ContainerId, ContainerLockGuard, ContainerRef},
        slots::ResultHandler,
    },
    player::Player,
    world::World,
};

/// Result slot handler for an anvil.
#[derive(Clone)]
pub struct AnvilResultHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    repair_item_count: Arc<AtomicI32>,
    level_cost: Arc<AtomicI32>,
    only_renaming: Arc<AtomicBool>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl AnvilResultHandler {
    /// Creates a new handler.
    pub const fn new(
        input_container: Shared<SimpleContainer>,
        result_container: Shared<ResultContainer>,
        repair_item_count: Arc<AtomicI32>,
        level_cost: Arc<AtomicI32>,
        only_renaming: Arc<AtomicBool>,
        block_pos: BlockPos,
        world: Arc<World>,
    ) -> Self {
        Self {
            input_container,
            result_container,
            repair_item_count,
            level_cost,
            only_renaming,
            block_pos,
            world,
        }
    }

    fn damage_anvil(&self, state: BlockStateId) {
        if let Some(new_state) = AnvilBlock::damage(state) {
            self.world
                .set_block(self.block_pos, new_state, UpdateFlags::UPDATE_CLIENTS);
            self.world
                .level_event(level_events::SOUND_ANVIL_USED, self.block_pos, 0, None);
        } else {
            self.world.remove_block(self.block_pos, false);
            self.world
                .level_event(level_events::SOUND_ANVIL_BROKEN, self.block_pos, 0, None);
        }
    }
}

impl ResultHandler for AnvilResultHandler {
    fn result_container(&self) -> ContainerRef {
        ContainerRef::from(self.result_container.clone())
    }

    fn dependencies(&self) -> Vec<ContainerRef> {
        vec![ContainerRef::from(self.input_container.clone())]
    }

    fn update_result(&self, _guard: &mut ContainerLockGuard) {}

    fn on_result_taken(
        &self,
        guard: &mut ContainerLockGuard,
        player: &Player,
    ) -> Option<ItemStack> {
        if !player.has_infinite_materials() {
            let mut experience = player.experience.lock();
            let cost = -self.level_cost.load(Ordering::Relaxed);
            experience.add_levels(cost);
        }

        let input_id = ContainerId::from_arc(&self.input_container);
        let input = guard.get_mut(input_id).expect("input container not locked");

        input.set_item(0, ItemStack::empty());

        let repair_cost = self.repair_item_count.load(Ordering::Relaxed);
        if repair_cost > 0 {
            let second = input.get_item_mut(1);
            if !second.is_empty() && second.count() > repair_cost {
                second.shrink(repair_cost);
            } else {
                input.set_item(1, ItemStack::empty());
            }
        } else if !self.only_renaming.load(Ordering::Relaxed) {
            input.set_item(1, ItemStack::empty());
        }

        self.level_cost.store(0, Ordering::Relaxed);

        let state = self.world.get_block_state(self.block_pos);
        if !player.has_infinite_materials()
            && REGISTRY
                .blocks
                .is_in_tag(state.get_block(), &BlockTag::ANVIL)
            && rand::random_bool(0.12)
        {
            self.damage_anvil(state);
        } else {
            self.world
                .level_event(level_events::SOUND_ANVIL_USED, self.block_pos, 0, None);
        }

        input.set_changed();
        guard
            .get_mut(ContainerId::from_arc(&self.result_container))
            .expect("container not locked")
            .set_changed();
        None
    }

    fn is_result_valid(&self, _guard: &ContainerLockGuard, player: &Player) -> bool {
        let level_cost = self.level_cost.load(Ordering::Relaxed);
        (player.has_infinite_materials() || player.experience.lock().level() >= level_cost)
            && level_cost > 0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32},
    };

    use steel_registry::{
        blocks::{block_state_ext::BlockStateExt as _, properties::BlockStateProperties},
        init_vanilla_registry, vanilla_blocks,
    };
    use steel_utils::{BlockPos, ChunkPos, locks::IntoShared as _, types::UpdateFlags};

    use super::AnvilResultHandler;
    use crate::{
        behavior::init_behaviors,
        inventory::container::{ResultContainer, SimpleContainer},
        test_support::{fresh_test_world, insert_ready_full_chunk},
        world::SignalGetter as _,
    };

    #[test]
    fn anvil_damage_does_not_notify_neighbors() {
        init_vanilla_registry();
        init_behaviors();
        let world = fresh_test_world("anvil_damage_update_flags");
        let anvil_pos = BlockPos::new(8, 64, 8);
        let lamp_pos = anvil_pos.east();
        let power_pos = lamp_pos.east();
        insert_ready_full_chunk(&world, ChunkPos::from_block_pos(anvil_pos));

        assert!(world.set_block(
            power_pos,
            vanilla_blocks::REDSTONE_BLOCK.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        ));
        assert!(world.set_block(
            lamp_pos,
            vanilla_blocks::REDSTONE_LAMP.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        ));
        assert!(world.set_block(
            anvil_pos,
            vanilla_blocks::ANVIL.default_state(),
            UpdateFlags::UPDATE_CLIENTS,
        ));
        assert!(world.has_neighbor_signal(lamp_pos));
        assert!(
            !world
                .get_block_state(lamp_pos)
                .get_value(&BlockStateProperties::LIT)
        );

        let handler = AnvilResultHandler::new(
            SimpleContainer::new(2).into_shared(),
            ResultContainer::new().into_shared(),
            Arc::new(AtomicI32::new(0)),
            Arc::new(AtomicI32::new(0)),
            Arc::new(AtomicBool::new(false)),
            anvil_pos,
            Arc::clone(&world),
        );
        handler.damage_anvil(world.get_block_state(anvil_pos));

        assert_eq!(
            world.get_block_state(anvil_pos).get_block(),
            &vanilla_blocks::CHIPPED_ANVIL
        );
        assert!(
            !world
                .get_block_state(lamp_pos)
                .get_value(&BlockStateProperties::LIT)
        );
    }
}
