//! Default spawn armor tiers shared by mobs.

use std::sync::LazyLock;

use steel_registry::items::ItemRef;
use steel_registry::vanilla_items;

use crate::inventory::equipment::EquipmentSlot;

/// One tier of default spawn armor.
#[derive(Clone, Copy, Debug)]
pub struct SpawnArmorTier {
    head: ItemRef,
    chest: ItemRef,
    legs: ItemRef,
    feet: ItemRef,
}

impl SpawnArmorTier {
    /// Creates a tier from the armor piece used in each armor slot.
    #[must_use]
    pub const fn new(head: ItemRef, chest: ItemRef, legs: ItemRef, feet: ItemRef) -> Self {
        Self {
            head,
            chest,
            legs,
            feet,
        }
    }

    /// Returns the armor piece this tier uses in `slot`.
    #[must_use]
    pub const fn item_for(self, slot: EquipmentSlot) -> Option<ItemRef> {
        match slot {
            EquipmentSlot::Head => Some(self.head),
            EquipmentSlot::Chest => Some(self.chest),
            EquipmentSlot::Legs => Some(self.legs),
            EquipmentSlot::Feet => Some(self.feet),
            EquipmentSlot::MainHand
            | EquipmentSlot::OffHand
            | EquipmentSlot::Body
            | EquipmentSlot::Saddle => None,
        }
    }
}

/// Vanilla `Mob.getEquipmentForSlot` armor tiers, ordered from leather to diamond.
pub static VANILLA_SPAWN_ARMOR_TIERS: LazyLock<[SpawnArmorTier; 6]> = LazyLock::new(|| {
    [
        SpawnArmorTier::new(
            &vanilla_items::LEATHER_HELMET,
            &vanilla_items::LEATHER_CHESTPLATE,
            &vanilla_items::LEATHER_LEGGINGS,
            &vanilla_items::LEATHER_BOOTS,
        ),
        SpawnArmorTier::new(
            &vanilla_items::COPPER_HELMET,
            &vanilla_items::COPPER_CHESTPLATE,
            &vanilla_items::COPPER_LEGGINGS,
            &vanilla_items::COPPER_BOOTS,
        ),
        SpawnArmorTier::new(
            &vanilla_items::GOLDEN_HELMET,
            &vanilla_items::GOLDEN_CHESTPLATE,
            &vanilla_items::GOLDEN_LEGGINGS,
            &vanilla_items::GOLDEN_BOOTS,
        ),
        SpawnArmorTier::new(
            &vanilla_items::CHAINMAIL_HELMET,
            &vanilla_items::CHAINMAIL_CHESTPLATE,
            &vanilla_items::CHAINMAIL_LEGGINGS,
            &vanilla_items::CHAINMAIL_BOOTS,
        ),
        SpawnArmorTier::new(
            &vanilla_items::IRON_HELMET,
            &vanilla_items::IRON_CHESTPLATE,
            &vanilla_items::IRON_LEGGINGS,
            &vanilla_items::IRON_BOOTS,
        ),
        SpawnArmorTier::new(
            &vanilla_items::DIAMOND_HELMET,
            &vanilla_items::DIAMOND_CHESTPLATE,
            &vanilla_items::DIAMOND_LEGGINGS,
            &vanilla_items::DIAMOND_BOOTS,
        ),
    ]
});
