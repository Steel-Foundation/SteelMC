//! Shared equipment slot definitions.

/// Equipment slot types for categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlotType {
    /// Hand slots (main hand, off hand).
    Hand,
    /// Humanoid armor slots (head, chest, legs, feet).
    HumanoidArmor,
    /// Animal armor slot (body).
    AnimalArmor,
    /// Saddle slot.
    Saddle,
}

/// Equipment slots for entities.
///
/// Based on Minecraft's `EquipmentSlot` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlot {
    /// The main hand slot.
    MainHand,
    /// The off hand slot.
    OffHand,
    /// The feet armor slot (boots).
    Feet,
    /// The legs armor slot (leggings).
    Legs,
    /// The chest armor slot (chestplate).
    Chest,
    /// The head armor slot (helmet).
    Head,
    /// The body armor slot (for animals like horses).
    Body,
    /// The saddle slot (for rideable animals).
    Saddle,
}

impl EquipmentSlot {
    /// All equipment slots in order.
    pub const ALL: [EquipmentSlot; 8] = [
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Feet,
        EquipmentSlot::Legs,
        EquipmentSlot::Chest,
        EquipmentSlot::Head,
        EquipmentSlot::Body,
        EquipmentSlot::Saddle,
    ];

    /// Humanoid armor slots (head, chest, legs, feet).
    pub const ARMOR_SLOTS: [EquipmentSlot; 4] = [
        EquipmentSlot::Head,
        EquipmentSlot::Chest,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
    ];

    /// Returns the slot type for this equipment slot.
    #[must_use]
    pub const fn slot_type(self) -> EquipmentSlotType {
        match self {
            EquipmentSlot::MainHand | EquipmentSlot::OffHand => EquipmentSlotType::Hand,
            EquipmentSlot::Feet
            | EquipmentSlot::Legs
            | EquipmentSlot::Chest
            | EquipmentSlot::Head => EquipmentSlotType::HumanoidArmor,
            EquipmentSlot::Body => EquipmentSlotType::AnimalArmor,
            EquipmentSlot::Saddle => EquipmentSlotType::Saddle,
        }
    }

    /// Returns vanilla `EquipmentSlot.canIncreaseExperience`.
    #[must_use]
    pub const fn can_increase_experience(self) -> bool {
        !matches!(self.slot_type(), EquipmentSlotType::Saddle)
    }

    /// Returns the index of this slot for array storage (0-7).
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            EquipmentSlot::MainHand => 0,
            EquipmentSlot::OffHand => 1,
            EquipmentSlot::Feet => 2,
            EquipmentSlot::Legs => 3,
            EquipmentSlot::Chest => 4,
            EquipmentSlot::Head => 5,
            EquipmentSlot::Body => 6,
            EquipmentSlot::Saddle => 7,
        }
    }

    /// Returns vanilla's protocol ID for this slot.
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            EquipmentSlot::MainHand => 0,
            EquipmentSlot::OffHand => 5,
            EquipmentSlot::Feet => 1,
            EquipmentSlot::Legs => 2,
            EquipmentSlot::Chest => 3,
            EquipmentSlot::Head => 4,
            EquipmentSlot::Body => 6,
            EquipmentSlot::Saddle => 7,
        }
    }

    /// Returns the equipment slot with the given vanilla protocol ID.
    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => EquipmentSlot::Feet,
            2 => EquipmentSlot::Legs,
            3 => EquipmentSlot::Chest,
            4 => EquipmentSlot::Head,
            5 => EquipmentSlot::OffHand,
            6 => EquipmentSlot::Body,
            7 => EquipmentSlot::Saddle,
            _ => EquipmentSlot::MainHand,
        }
    }

    /// Returns true if this is an armor slot (humanoid or animal).
    #[must_use]
    pub const fn is_armor(self) -> bool {
        matches!(
            self.slot_type(),
            EquipmentSlotType::HumanoidArmor | EquipmentSlotType::AnimalArmor
        )
    }

    /// Returns the equipment slot with the given name, or None if not found.
    #[must_use]
    pub const fn by_name(name: &str) -> Option<Self> {
        match name {
            "mainhand" => Some(EquipmentSlot::MainHand),
            "offhand" => Some(EquipmentSlot::OffHand),
            "feet" => Some(EquipmentSlot::Feet),
            "legs" => Some(EquipmentSlot::Legs),
            "chest" => Some(EquipmentSlot::Chest),
            "head" => Some(EquipmentSlot::Head),
            "body" => Some(EquipmentSlot::Body),
            "saddle" => Some(EquipmentSlot::Saddle),
            _ => None,
        }
    }

    /// Returns the name of this equipment slot.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            EquipmentSlot::MainHand => "mainhand",
            EquipmentSlot::OffHand => "offhand",
            EquipmentSlot::Feet => "feet",
            EquipmentSlot::Legs => "legs",
            EquipmentSlot::Chest => "chest",
            EquipmentSlot::Head => "head",
            EquipmentSlot::Body => "body",
            EquipmentSlot::Saddle => "saddle",
        }
    }
}

/// Equipment slot groups used by vanilla item attributes, loot, and enchantments.
///
/// Vanilla's `EquipmentSlotGroup` is a predicate over concrete equipment slots:
/// `Hand` matches both hand slots, `Armor` matches humanoid and animal armor,
/// and `Any` matches every slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlotGroup {
    Any,
    MainHand,
    OffHand,
    Hand,
    Feet,
    Legs,
    Chest,
    Head,
    Armor,
    Body,
    Saddle,
}

impl EquipmentSlotGroup {
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::Any => 0,
            Self::MainHand => 1,
            Self::OffHand => 2,
            Self::Hand => 3,
            Self::Feet => 4,
            Self::Legs => 5,
            Self::Chest => 6,
            Self::Head => 7,
            Self::Armor => 8,
            Self::Body => 9,
            Self::Saddle => 10,
        }
    }

    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::MainHand,
            2 => Self::OffHand,
            3 => Self::Hand,
            4 => Self::Feet,
            5 => Self::Legs,
            6 => Self::Chest,
            7 => Self::Head,
            8 => Self::Armor,
            9 => Self::Body,
            10 => Self::Saddle,
            _ => Self::Any,
        }
    }

    #[must_use]
    pub const fn by_name(name: &str) -> Option<Self> {
        match name {
            "any" => Some(Self::Any),
            "mainhand" | "main_hand" => Some(Self::MainHand),
            "offhand" | "off_hand" => Some(Self::OffHand),
            "hand" => Some(Self::Hand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "armor" => Some(Self::Armor),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "any",
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Hand => "hand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Armor => "armor",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.name()
    }

    #[must_use]
    pub const fn by_slot(slot: EquipmentSlot) -> Self {
        match slot {
            EquipmentSlot::MainHand => Self::MainHand,
            EquipmentSlot::OffHand => Self::OffHand,
            EquipmentSlot::Feet => Self::Feet,
            EquipmentSlot::Legs => Self::Legs,
            EquipmentSlot::Chest => Self::Chest,
            EquipmentSlot::Head => Self::Head,
            EquipmentSlot::Body => Self::Body,
            EquipmentSlot::Saddle => Self::Saddle,
        }
    }

    #[must_use]
    pub const fn test(self, slot: EquipmentSlot) -> bool {
        match self {
            Self::Any => true,
            Self::MainHand => matches!(slot, EquipmentSlot::MainHand),
            Self::OffHand => matches!(slot, EquipmentSlot::OffHand),
            Self::Hand => matches!(slot.slot_type(), EquipmentSlotType::Hand),
            Self::Feet => matches!(slot, EquipmentSlot::Feet),
            Self::Legs => matches!(slot, EquipmentSlot::Legs),
            Self::Chest => matches!(slot, EquipmentSlot::Chest),
            Self::Head => matches!(slot, EquipmentSlot::Head),
            Self::Armor => slot.is_armor(),
            Self::Body => matches!(slot, EquipmentSlot::Body),
            Self::Saddle => matches!(slot, EquipmentSlot::Saddle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::EquipmentSlot;

    #[test]
    fn equipment_slot_protocol_ids_match_vanilla() {
        let slots = [
            (EquipmentSlot::MainHand, 0),
            (EquipmentSlot::Feet, 1),
            (EquipmentSlot::Legs, 2),
            (EquipmentSlot::Chest, 3),
            (EquipmentSlot::Head, 4),
            (EquipmentSlot::OffHand, 5),
            (EquipmentSlot::Body, 6),
            (EquipmentSlot::Saddle, 7),
        ];

        for (slot, id) in slots {
            assert_eq!(slot.id(), id);
            assert_eq!(EquipmentSlot::by_id(id), slot);
        }
        assert_eq!(EquipmentSlot::by_id(-1), EquipmentSlot::MainHand);
        assert_eq!(EquipmentSlot::by_id(8), EquipmentSlot::MainHand);
    }

    #[test]
    fn only_saddle_slot_does_not_increase_experience() {
        for slot in EquipmentSlot::ALL {
            assert_eq!(
                slot.can_increase_experience(),
                slot != EquipmentSlot::Saddle
            );
        }
    }
}
