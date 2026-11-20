//! Difficulty-scaled armor for newly spawned mobs.

use std::sync::LazyLock;

use steel_registry::item_stack::ItemStack;
use steel_registry::items::ItemRef;
use steel_registry::vanilla_items;
use steel_utils::types::Difficulty;

use super::Parts;
use crate::inventory::equipment::EquipmentSlot;
use crate::world::DifficultyInstance;

const INITIAL_ARMOR_TIER_COUNT: usize = 3;
const MAX_WEARING_ARMOR_CHANCE: f32 = 0.15;
const WEARING_ARMOR_UPGRADE_MATERIAL_CHANCE: f32 = 0.1087;
const WEARING_ARMOR_UPGRADE_MATERIAL_ATTEMPTS: i32 = 3;
const HARD_ARMOR_PARTIAL_CHANCE: f32 = 0.1;
const OTHER_ARMOR_PARTIAL_CHANCE: f32 = 0.25;

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

/// Equips empty armor slots in order, starting with the helmet.
pub fn populate_default_equipment_slots<E: Parts>(entity: &E, difficulty: DifficultyInstance) {
    if rand::random::<f32>() >= MAX_WEARING_ARMOR_CHANCE * difficulty.special_multiplier() {
        return;
    }

    let mut tier_index = rand::random_range(0..INITIAL_ARMOR_TIER_COUNT);
    for _ in 0..WEARING_ARMOR_UPGRADE_MATERIAL_ATTEMPTS {
        if rand::random::<f32>() < WEARING_ARMOR_UPGRADE_MATERIAL_CHANCE {
            tier_index += 1;
        }
    }

    let tier = VANILLA_SPAWN_ARMOR_TIERS[tier_index];
    let partial_chance = if difficulty.difficulty() == Difficulty::Hard {
        HARD_ARMOR_PARTIAL_CHANCE
    } else {
        OTHER_ARMOR_PARTIAL_CHANCE
    };

    let mob = entity.mob();
    let mut equipment = mob.living_base().equipment().lock();

    for (index, slot) in EquipmentSlot::ARMOR_SLOTS.into_iter().enumerate() {
        if index > 0 && rand::random::<f32>() < partial_chance {
            break;
        }

        if equipment.get_ref(slot).is_empty()
            && let Some(item) = tier.item_for(slot)
        {
            equipment.set(slot, ItemStack::new(item));
        }
    }
}
