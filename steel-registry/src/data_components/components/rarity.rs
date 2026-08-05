//! Vanilla `minecraft:rarity` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::format::Color;

/// Vanilla item rarity, including its serialized and network IDs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Rarity {
    #[default]
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Epic => "epic",
        }
    }

    #[must_use]
    pub const fn color(self) -> Color {
        match self {
            Self::Common => Color::White,
            Self::Uncommon => Color::Yellow,
            Self::Rare => Color::Aqua,
            Self::Epic => Color::LightPurple,
        }
    }

    const fn network_id(self) -> i32 {
        match self {
            Self::Common => 0,
            Self::Uncommon => 1,
            Self::Rare => 2,
            Self::Epic => 3,
        }
    }

    const fn from_network_id(id: i32) -> Self {
        match id {
            1 => Self::Uncommon,
            2 => Self::Rare,
            3 => Self::Epic,
            _ => Self::Common,
        }
    }

    const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "common" => Some(Self::Common),
            "uncommon" => Some(Self::Uncommon),
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            _ => None,
        }
    }
}

impl WriteTo for Rarity {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.network_id()).write(writer)
    }
}

impl ReadFrom for Rarity {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::from_network_id(VarInt::read(data)?.0))
    }
}

impl ToNbtTag for Rarity {
    fn to_nbt_tag(self) -> NbtTag {
        self.serialized_name().to_nbt_tag()
    }
}

impl FromNbtTag for Rarity {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_serialized_name(&tag.string()?.to_str())
    }
}

impl HashComponent for Rarity {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(self.serialized_name());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::Rarity;

    #[test]
    fn network_ids_match_vanilla_and_fall_back_to_common() {
        for (rarity, expected_id) in [
            (Rarity::Common, 0),
            (Rarity::Uncommon, 1),
            (Rarity::Rare, 2),
            (Rarity::Epic, 3),
        ] {
            let mut encoded = Vec::new();
            rarity.write(&mut encoded).expect("rarity should encode");
            let mut cursor = Cursor::new(encoded.as_slice());
            assert_eq!(
                VarInt::read(&mut cursor).expect("id should decode").0,
                expected_id
            );
        }

        for id in [-1, 4, i32::MAX] {
            let mut encoded = Vec::new();
            VarInt(id).write(&mut encoded).expect("id should encode");
            assert_eq!(
                Rarity::read(&mut Cursor::new(encoded.as_slice())).expect("rarity should decode"),
                Rarity::Common
            );
        }
    }
}
