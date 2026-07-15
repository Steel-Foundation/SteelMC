//! Vanilla `minecraft:swing_animation` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Visual animation used when an item is swung.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SwingAnimationType {
    None,
    #[default]
    Whack,
    Stab,
}

impl SwingAnimationType {
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Whack => "whack",
            Self::Stab => "stab",
        }
    }

    const fn network_id(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Whack => 1,
            Self::Stab => 2,
        }
    }

    const fn from_network_id(id: i32) -> Self {
        match id {
            1 => Self::Whack,
            2 => Self::Stab,
            _ => Self::None,
        }
    }

    const fn from_serialized_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "whack" => Some(Self::Whack),
            "stab" => Some(Self::Stab),
            _ => None,
        }
    }
}

impl WriteTo for SwingAnimationType {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.network_id()).write(writer)
    }
}

impl ReadFrom for SwingAnimationType {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::from_network_id(VarInt::read(data)?.0))
    }
}

impl HashComponent for SwingAnimationType {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(self.serialized_name());
    }
}

/// Vanilla swing animation type and duration in ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwingAnimation {
    pub animation_type: SwingAnimationType,
    pub duration: i32,
}

impl SwingAnimation {
    pub const DEFAULT: Self = Self {
        animation_type: SwingAnimationType::Whack,
        duration: 6,
    };

    #[must_use]
    pub const fn new(animation_type: SwingAnimationType, duration: i32) -> Self {
        Self {
            animation_type,
            duration,
        }
    }
}

impl Default for SwingAnimation {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl WriteTo for SwingAnimation {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.animation_type.write(writer)?;
        VarInt(self.duration).write(writer)
    }
}

impl ReadFrom for SwingAnimation {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            animation_type: SwingAnimationType::read(data)?,
            duration: VarInt::read(data)?.0,
        })
    }
}

impl ToNbtTag for SwingAnimation {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.animation_type != Self::DEFAULT.animation_type {
            compound.insert("type", self.animation_type.serialized_name());
        }
        if self.duration != Self::DEFAULT.duration {
            compound.insert("duration", self.duration);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for SwingAnimation {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let animation_type = match compound.get("type") {
            Some(tag) => SwingAnimationType::from_serialized_name(&tag.string()?.to_str())?,
            None => Self::DEFAULT.animation_type,
        };
        let duration = compound
            .get("duration")
            .map_or(Some(Self::DEFAULT.duration), |tag| tag.codec_i32())?;
        (duration > 0).then_some(Self {
            animation_type,
            duration,
        })
    }
}

impl HashComponent for SwingAnimation {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if self.animation_type != Self::DEFAULT.animation_type {
            push_hash_entry(&mut entries, "type", &self.animation_type);
        }
        if self.duration != Self::DEFAULT.duration {
            push_hash_entry(&mut entries, "duration", &self.duration);
        }
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::{SwingAnimation, SwingAnimationType};

    fn parse(tag: NbtTag) -> Option<SwingAnimation> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        SwingAnimation::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn empty_compound_uses_vanilla_default() {
        assert_eq!(
            parse(NbtTag::Compound(NbtCompound::new())),
            Some(SwingAnimation::DEFAULT)
        );
    }

    #[test]
    fn persistent_duration_must_be_positive() {
        for duration in [0, -1] {
            let mut compound = NbtCompound::new();
            compound.insert("duration", duration);
            assert_eq!(parse(NbtTag::Compound(compound)), None);
        }
    }

    #[test]
    fn network_type_ids_fall_back_to_none() {
        for id in [-1, 3, i32::MAX] {
            let mut encoded = Vec::new();
            VarInt(id).write(&mut encoded).expect("id should encode");
            assert_eq!(
                SwingAnimationType::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("animation type should decode"),
                SwingAnimationType::None
            );
        }
    }
}
