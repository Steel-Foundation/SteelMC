//! Furnace-specific input, fuel, and result slots.

use steel_registry::item_stack::ItemStack;
use steel_registry::vanilla_items;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::block_entity::entities::{FurnaceContainer, pop_furnace_experience};
use crate::entity::Entity as _;
use crate::inventory::fuel_values::VANILLA_FUEL_VALUES;
use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::slots::{NormalSlot, Slot, SlotStorage};
use crate::player::Player;

/// Accepts furnace fuel and empty buckets, with Vanilla's bucket stack limit.
pub struct FurnaceFuelSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies furnace fuel slots.
unsafe impl DowncastType for FurnaceFuelSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/furnace_fuel");
}

impl FurnaceFuelSlot {
    /// Creates a furnace fuel slot.
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
        VANILLA_FUEL_VALUES.is_fuel(stack.item()) || stack.is(&vanilla_items::BUCKET)
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

/// Rejects insertion and awards accumulated cooking XP when output is taken.
pub struct FurnaceResultSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies furnace result slots.
unsafe impl DowncastType for FurnaceResultSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/furnace_result");
}

impl FurnaceResultSlot {
    /// Creates a furnace result slot.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
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
        // TODO: Award crafted stats/recipes and trigger RECIPE_CRAFTED once supported.
        let (container, _) = self.storage().physical_backing()?;
        let container_id = container.container_id();
        let recipes = guard
            .get_typed_mut::<FurnaceContainer>(container_id)
            .map(FurnaceContainer::take_recipes_used);
        self.base.set_changed(guard);
        if let Some(recipes) = recipes {
            guard.run_unlocked(|| {
                pop_furnace_experience(&player.get_world(), player.position(), recipes);
            });
        }
        None
    }
}
