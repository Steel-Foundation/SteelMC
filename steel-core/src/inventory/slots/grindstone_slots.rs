//! Grindstone Menu
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, Ordering},
};

use steel_registry::{
    REGISTRY, RegistryExt, TaggedRegistryExt,
    blocks::block_state_ext::BlockStateExt,
    data_components::{
        components::ItemEnchantments,
        vanilla_components::{CUSTOM_NAME, ENCHANTMENTS, REPAIR_COST, STORED_ENCHANTMENTS},
    },
    enchantment::Enchantment,
    item_stack::ItemStack,
    vanilla_block_tags::BlockTag,
    vanilla_items, vanilla_menu_types,
};
use steel_utils::{
    BlockPos, Identifier, java,
    locks::{IntoShared, Shared, SyncMutex},
    text::DisplayResolutor,
};
use text_components::TextComponent;

use crate::{
    behavior::ITEM_BEHAVIORS,
    inventory::{
        container::{ResultContainer, SimpleContainer},
        prelude::*,
        slots::AnvilResultHandler,
    },
    player::player_inventory::PlayerInventory,
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
        player: &Player,
    ) -> Option<ItemStack> {
        None
    }

    fn is_result_valid(&self, _guard: &ContainerLockGuard, player: &Player) -> bool {
        true
    }
}
