//! Barrel block entity.
use crate::{
    block_entity::{
        BlockEntity, BlockEntityBase, RandomizableContainer,
        randomizable_container::RandomizableContainerStorage,
    },
    inventory::lock::ContainerRef,
    loot::LootResult,
    player::Player,
    world::World,
};
use simdnbt::{borrow::BaseNbtCompound, owned::NbtCompound};
use std::sync::Weak;
use steel_registry::{
    data_components::vanilla_components::SeededContainerLoot, item_stack::ItemStack,
    vanilla_block_entity_types,
};
use steel_utils::{BlockPos, BlockStateId, DowncastType, DowncastTypeKey};

/// Number of slots in a barrel.
pub const BARREL_SLOTS: usize = 27;

/// A barrel's independent inventory and lazy loot storage.
pub struct BarrelBlockEntity {
    storage: RandomizableContainerStorage,
}

// SAFETY: This Steel-owned key uniquely identifies `BarrelBlockEntity`.
unsafe impl DowncastType for BarrelBlockEntity {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:block_entity/barrel");
}

impl BarrelBlockEntity {
    /// Creates a barrel with independent inventory storage.
    #[must_use]
    pub fn new(level: Weak<World>, pos: BlockPos, state: BlockStateId) -> Self {
        Self {
            storage: RandomizableContainerStorage::new(
                &vanilla_block_entity_types::BARREL,
                BARREL_SLOTS,
                level,
                pos,
                state,
            ),
        }
    }
}

impl BlockEntity for BarrelBlockEntity {
    fn base(&self) -> &BlockEntityBase {
        &self.storage.base
    }
    fn load_additional(&self, nbt: &BaseNbtCompound<'_>) {
        self.storage.load_additional(nbt);
    }
    fn save_additional(&self, nbt: &mut NbtCompound) {
        self.storage.save_additional(nbt);
    }
    fn pre_remove_side_effects(&self, pos: BlockPos, _state: BlockStateId) {
        self.storage.pre_remove_side_effects(pos);
    }
    fn apply_components_from_item(&self, item: &ItemStack) {
        self.storage.apply_components_from_item(item);
    }
    fn container_ref(&self) -> Option<ContainerRef> {
        Some(self.storage.container_ref.clone())
    }
    fn as_randomizable_container(&self) -> Option<&dyn RandomizableContainer> {
        Some(self)
    }
}

impl RandomizableContainer for BarrelBlockEntity {
    fn loot_table(&self) -> Option<SeededContainerLoot> {
        self.storage.loot_table()
    }
    fn set_loot_table(&self, loot: Option<SeededContainerLoot>) {
        self.storage.set_loot_table(loot);
    }
    fn unpack_loot_table(&self, player: Option<&Player>) -> LootResult<()> {
        self.storage.unpack_loot_table(player)
    }
}

#[cfg(test)]
#[path = "barrel/tests.rs"]
mod tests;
