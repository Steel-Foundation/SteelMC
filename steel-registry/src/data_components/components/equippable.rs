//! Equippable component for armor and equipment items.

use std::io::{Result, Write};

use steel_utils::{
    hash::{ComponentHasher, HashComponent},
    serial::{ReadFrom, WriteTo},
};

/// Equipment slot for the equippable component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquippableSlot {
    Head,
    Chest,
    Legs,
    Feet,
    Body,
    Mainhand,
    Offhand,
    Saddle,
}

impl EquippableSlot {
    /// Parses an equipment slot from a string (as used in items.json).
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "head" => Some(Self::Head),
            "chest" => Some(Self::Chest),
            "legs" => Some(Self::Legs),
            "feet" => Some(Self::Feet),
            "body" => Some(Self::Body),
            "mainhand" => Some(Self::Mainhand),
            "offhand" => Some(Self::Offhand),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }

    /// Returns the string representation of this slot.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Head => "head",
            Self::Chest => "chest",
            Self::Legs => "legs",
            Self::Feet => "feet",
            Self::Body => "body",
            Self::Mainhand => "mainhand",
            Self::Offhand => "offhand",
            Self::Saddle => "saddle",
        }
    }

    /// Returns true if this is a humanoid armor slot.
    #[must_use]
    pub const fn is_humanoid_armor(&self) -> bool {
        matches!(self, Self::Head | Self::Chest | Self::Legs | Self::Feet)
    }
}

/// The equippable component data.
#[derive(Debug, Clone, PartialEq)]
pub struct Equippable {
    pub slot: EquippableSlot,
}

impl WriteTo for Equippable {
    fn write(&self, _writer: &mut impl Write) -> Result<()> {
        // TODO: Implement proper Equippable serialization
        // Format: slot (VarInt), equip_sound (SoundEvent), model (Optional), camera_overlay (Optional),
        //         allowed_entities (Optional HolderSet), dispensable (bool), swappable (bool),
        //         damage_on_hurt (bool), equip_on_interact (bool)
        Ok(())
    }
}

impl ReadFrom for Equippable {
    fn read(_data: &mut std::io::Cursor<&[u8]>) -> Result<Self> {
        // TODO: Implement proper Equippable deserialization
        Ok(Self {
            slot: EquippableSlot::Chest,
        })
    }
}

impl HashComponent for Equippable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        // Equippable is hashed as a map
        // For now, hash as empty map since full implementation requires proper codec
        hasher.start_map();
        // TODO: Add proper field hashing when Equippable codec is implemented
        hasher.end_map();
    }
}

impl simdnbt::ToNbtTag for Equippable {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};

        let mut compound = NbtCompound::new();
        compound.insert("slot", self.slot.as_str());
        NbtTag::Compound(compound)
    }
}

impl simdnbt::FromNbtTag for Equippable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let slot_str = compound.get("slot")?.string()?.to_str();
        let slot = EquippableSlot::parse(&slot_str)?;
        Some(Self { slot })
    }
}
