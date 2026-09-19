use steel_registry::item_stack::ItemStack;

use crate::entity::LivingEntity;
use crate::inventory::equipment::EquipmentSlot;

/// Leaves equipment unlocked during effects, which may inspect it during retaliation.
pub(super) enum EnchantedItemInUse<'a> {
    Stack(&'a mut ItemStack),
    Equipped {
        owner: &'a dyn LivingEntity,
        slot: EquipmentSlot,
    },
}

impl EnchantedItemInUse<'_> {
    pub(super) fn with_item(&self, visitor: &mut dyn FnMut(&ItemStack)) {
        match self {
            Self::Stack(stack) => visitor(stack),
            Self::Equipped { owner, slot } => owner.with_equipment_slot(*slot, visitor),
        }
    }

    pub(super) fn hurt_and_break(&mut self, amount: i32, infinite_materials: bool) -> bool {
        match self {
            Self::Stack(stack) => stack.hurt_and_break(amount, infinite_materials),
            Self::Equipped { owner, slot } => {
                let mut broke = false;
                owner.with_equipment_slot_mut(*slot, &mut |stack| {
                    broke = stack.hurt_and_break(amount, infinite_materials);
                });
                broke
            }
        }
    }
}
