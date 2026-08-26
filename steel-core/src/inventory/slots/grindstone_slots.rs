//! Grindstone Menu
use std::sync::Arc;

use steel_registry::{item_stack::ItemStack, level_events};
use steel_utils::{BlockPos, locks::Shared};

use crate::{
    entity::entities::ExperienceOrbEntity,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
    },
    world::World,
};

/// Result slot handler for an grindstone.
#[derive(Clone)]
pub struct GrindstoneResultHandler {
    input_container: Shared<SimpleContainer>,
    result_container: Shared<ResultContainer>,
    block_pos: BlockPos,
    world: Arc<World>,
}

impl GrindstoneResultHandler {
    /// Creates a new handler.
    pub const fn new(
        input_container: Shared<SimpleContainer>,
        result_container: Shared<ResultContainer>,
        block_pos: BlockPos,
        world: Arc<World>,
    ) -> Self {
        Self {
            input_container,
            result_container,
            block_pos,
            world,
        }
    }
}

impl ResultHandler for GrindstoneResultHandler {
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
        _player: &Player,
    ) -> Option<ItemStack> {
        // TODO: Add offset for orb spawning
        // TODO: Implement experience orb calculation
        ExperienceOrbEntity::award(&self.world, self.block_pos.as_dvec3(), 1);

        self.world
            .level_event(level_events::SOUND_GRINDSTONE_USED, self.block_pos, 0, None);

        let id = ContainerId::from_arc(&self.input_container);
        let Some(input) = guard.get_mut(id) else {
            log::warn!("Couldn't get lock for grindstone.");
            return None;
        };

        input.set_item(0, ItemStack::empty());
        input.set_item(1, ItemStack::empty());

        None
    }

    fn is_result_valid(&self, _guard: &ContainerLockGuard, _player: &Player) -> bool {
        true
    }
}
