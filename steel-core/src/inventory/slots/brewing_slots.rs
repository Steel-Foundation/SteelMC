//! Brewing-stand potion, ingredient, and fuel slots.

use steel_registry::REGISTRY;
use steel_registry::data_components::vanilla_components::BREWING_FUEL;
use steel_registry::item_stack::ItemStack;
use steel_registry::recipe::PotionIngredient;
use steel_utils::{DowncastType, DowncastTypeKey};

use crate::inventory::lock::{ContainerLockGuard, ContainerRef};
use crate::inventory::slots::{NormalSlot, Slot, SlotStorage};
use crate::player::Player;

/// Accepts only brewing-stand potion inputs and holds one item.
pub struct PotionSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies brewing-stand potion slots.
unsafe impl DowncastType for PotionSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/brewing_potion");
}

impl PotionSlot {
    /// Creates a brewing-stand potion slot.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            base: NormalSlot::new(container, index),
        }
    }
}

impl Slot for PotionSlot {
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
        PotionIngredient::is_potion_input(stack)
    }

    fn get_max_stack_size(&self, _guard: &ContainerLockGuard) -> i32 {
        1
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
        stack: &ItemStack,
        player: &Player,
    ) -> Option<ItemStack> {
        // TODO: Trigger `CriteriaTriggers.BREWED_POTION` once advancement
        // criteria support exists.
        self.base.on_take(guard, stack, player)
    }
}

/// Accepts only items used as reagents by a brewing recipe.
pub struct IngredientsSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies brewing-stand ingredient slots.
unsafe impl DowncastType for IngredientsSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/brewing_ingredient");
}

impl IngredientsSlot {
    /// Creates a brewing-stand ingredient slot.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            base: NormalSlot::new(container, index),
        }
    }
}

impl Slot for IngredientsSlot {
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
        REGISTRY.recipes.is_brewing_reagent(stack.item())
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
}

/// Accepts only items with a `minecraft:brewing_fuel` component.
pub struct FuelSlot {
    base: NormalSlot,
}

// SAFETY: This Steel-owned key uniquely identifies brewing-stand fuel slots.
unsafe impl DowncastType for FuelSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/brewing_fuel");
}

impl FuelSlot {
    /// Creates a brewing-stand fuel slot.
    #[must_use]
    pub fn new(container: impl Into<ContainerRef>, index: usize) -> Self {
        Self {
            base: NormalSlot::new(container, index),
        }
    }

    /// Returns whether `stack` is brewing fuel.
    #[must_use]
    pub fn may_place_item(stack: &ItemStack) -> bool {
        stack.has(BREWING_FUEL)
    }
}

impl Slot for FuelSlot {
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
        Self::may_place_item(stack)
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
}
