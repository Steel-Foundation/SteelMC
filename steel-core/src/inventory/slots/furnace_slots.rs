//! Slots with normal-furnace placement and result-taking behavior.

use steel_registry::{item_stack::ItemStack, vanilla_items};
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::block_entity::entities::FurnaceContainer;
use crate::block_entity::vanilla_fuel_values;
use crate::entity::Entity as _;
use crate::inventory::lock::{ContainerId, ContainerLockGuard, ContainerRef};
use crate::inventory::slots::{NormalSlot, Slot, SlotStorage};
use crate::player::Player;

/// Furnace fuel slot, including Vanilla's empty-bucket stack limit.
pub struct FurnaceFuelSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies `FurnaceFuelSlot`.
unsafe impl DowncastType for FurnaceFuelSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/furnace_fuel");
}

impl FurnaceFuelSlot {
    /// Creates a fuel slot over `index` in `container`.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            base: NormalSlot::new(container, index),
        }
    }
}

impl Slot for FurnaceFuelSlot {
    fn storage(&self) -> &SlotStorage {
        self.base.storage()
    }

    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
        self.base.get_item(guard)
    }

    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
        self.base.get_item_mut(guard)
    }

    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
        self.base.set_item(guard, stack);
    }

    fn may_place(&self, stack: &ItemStack) -> bool {
        vanilla_fuel_values().is_fuel(stack) || stack.is(&vanilla_items::BUCKET)
    }

    fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32 {
        self.base.get_max_stack_size(guard)
    }

    fn get_max_stack_size_for_item(&self, guard: &ContainerLockGuard, stack: &ItemStack) -> i32 {
        if stack.is(&vanilla_items::BUCKET) {
            1
        } else {
            self.base.get_max_stack_size_for_item(guard, stack)
        }
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        self.base.set_changed(guard);
    }

    fn get_container_slot(&self) -> usize {
        self.base.get_container_slot()
    }
}

/// Furnace output slot that pays accumulated recipe experience when taken.
pub struct FurnaceResultSlot {
    base: NormalSlot,
    container_id: ContainerId,
}

// SAFETY: This Steel-owned key uniquely identifies `FurnaceResultSlot`.
unsafe impl DowncastType for FurnaceResultSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/furnace_result");
}

impl FurnaceResultSlot {
    /// Creates a furnace result slot over `index` in `container`.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        let container = container.into();
        Self {
            container_id: container.container_id(),
            base: NormalSlot::new(container, index),
        }
    }
}

impl Slot for FurnaceResultSlot {
    fn storage(&self) -> &SlotStorage {
        self.base.storage()
    }

    fn get_item<'a>(&self, guard: &'a ContainerLockGuard) -> &'a ItemStack {
        self.base.get_item(guard)
    }

    fn get_item_mut<'a>(&self, guard: &'a mut ContainerLockGuard) -> &'a mut ItemStack {
        self.base.get_item_mut(guard)
    }

    fn set_item(&self, guard: &mut ContainerLockGuard, stack: ItemStack) {
        self.base.set_item(guard, stack);
    }

    fn may_place(&self, _stack: &ItemStack) -> bool {
        false
    }

    fn get_max_stack_size(&self, guard: &ContainerLockGuard) -> i32 {
        self.base.get_max_stack_size(guard)
    }

    fn set_changed(&self, guard: &mut ContainerLockGuard) {
        self.base.set_changed(guard);
    }

    fn get_container_slot(&self) -> usize {
        self.base.get_container_slot()
    }

    fn on_take(
        &self,
        guard: &mut ContainerLockGuard,
        _stack: &ItemStack,
        player: &Player,
    ) -> Option<ItemStack> {
        let recipes_used =
            if let Some(furnace) = guard.get_typed_mut::<FurnaceContainer>(self.container_id) {
                furnace.take_recipes_used()
            } else {
                Vec::new()
            };
        if !recipes_used.is_empty() {
            let world = player.get_world();
            let position = player.position();
            guard.run_unlocked(|| {
                FurnaceContainer::pop_recipe_experience(&world, position, recipes_used);
            });
        }

        // TODO: Run ItemStack.onCraftedBy and award/trigger used recipes once
        // Steel has the corresponding item and recipe-advancement foundations.
        self.base.set_changed(guard);
        None
    }
}
