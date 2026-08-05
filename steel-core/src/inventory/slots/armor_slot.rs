use crate::{
    inventory::{
        lock::{ContainerLockGuard, ContainerRef},
        slots::{NormalSlot, Slot, SlotStorage},
    },
    player::Player,
};
use steel_registry::{
    enchantment_effect::EnchantmentEffectComponent, equipment::EquipmentSlot, item_stack::ItemStack,
};
use steel_utils::{DowncastType, DowncastTypeKey};

/// A [`NormalSlot`] that only accepts items equippable in its equipment slot,
/// caps at one item, and respects the prevent-armor-change enchantment effect.
pub struct ArmorSlot {
    base: NormalSlot,
    slot: EquipmentSlot,
}

// SAFETY: This key uniquely identifies Steel's `ArmorSlot`.
unsafe impl DowncastType for ArmorSlot {
    const TYPE_KEY: DowncastTypeKey = DowncastTypeKey::new("steel:slot/armor");
}

impl ArmorSlot {
    /// Creates a new armor slot.
    pub fn new(container: impl Into<ContainerRef>, index: usize, slot: EquipmentSlot) -> Self {
        Self {
            base: NormalSlot::new(container, index),
            slot,
        }
    }

    /// Returns the equipment slot this armor slot accepts.
    #[must_use]
    pub const fn equipment_slot(&self) -> EquipmentSlot {
        self.slot
    }

    /// Returns a reference to the container.
    #[must_use]
    pub fn container_ref(&self) -> ContainerRef {
        self.base.container_ref()
    }
}

impl Slot for ArmorSlot {
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

    fn set_by_player(
        &self,
        guard: &mut ContainerLockGuard,
        stack: ItemStack,
        previous: &ItemStack,
    ) {
        // TODO: Call player.onEquipItem(equipmentSlot, previous, stack) here
        let _ = previous;
        self.set_item(guard, stack);
    }

    fn may_place(&self, stack: &ItemStack) -> bool {
        stack.is_equippable_in_slot(self.slot)
    }

    fn may_pickup(&self, guard: &ContainerLockGuard, player: &Player) -> bool {
        let item = self.get_item(guard);
        if !item.is_empty()
            && !player.has_infinite_materials()
            && item.has_enchantment_effect(EnchantmentEffectComponent::PreventArmorChange)
        {
            return false;
        }
        true
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
}
