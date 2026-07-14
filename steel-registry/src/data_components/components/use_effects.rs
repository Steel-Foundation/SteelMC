//! Vanilla `minecraft:use_effects` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

/// Controls movement and vibration behavior while an item is being used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UseEffects {
    pub can_sprint: bool,
    pub interact_vibrations: bool,
    pub speed_multiplier: f32,
}

impl UseEffects {
    pub const DEFAULT: Self = Self {
        can_sprint: false,
        interact_vibrations: true,
        speed_multiplier: 0.2,
    };

    #[must_use]
    pub const fn new(can_sprint: bool, interact_vibrations: bool, speed_multiplier: f32) -> Self {
        Self {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        }
    }
}

impl Default for UseEffects {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl WriteTo for UseEffects {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.can_sprint.write(writer)?;
        self.interact_vibrations.write(writer)?;
        self.speed_multiplier.write(writer)
    }
}

impl ReadFrom for UseEffects {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            can_sprint: bool::read(data)?,
            interact_vibrations: bool::read(data)?,
            speed_multiplier: f32::read(data)?,
        })
    }
}

impl ToNbtTag for UseEffects {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        if self.can_sprint != Self::DEFAULT.can_sprint {
            compound.insert("can_sprint", self.can_sprint);
        }
        if self.interact_vibrations != Self::DEFAULT.interact_vibrations {
            compound.insert("interact_vibrations", self.interact_vibrations);
        }
        if self.speed_multiplier.to_bits() != Self::DEFAULT.speed_multiplier.to_bits() {
            compound.insert("speed_multiplier", self.speed_multiplier);
        }
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for UseEffects {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let can_sprint = compound
            .get("can_sprint")
            .map_or(Some(Self::DEFAULT.can_sprint), |tag| tag.codec_bool())?;
        let interact_vibrations = compound
            .get("interact_vibrations")
            .map_or(Some(Self::DEFAULT.interact_vibrations), |tag| {
                tag.codec_bool()
            })?;
        let speed_multiplier = compound
            .get("speed_multiplier")
            .map_or(Some(Self::DEFAULT.speed_multiplier), |tag| tag.codec_f32())?;
        if !speed_multiplier.is_finite() || !(0.0..=1.0).contains(&speed_multiplier) {
            return None;
        }
        Some(Self {
            can_sprint,
            interact_vibrations,
            speed_multiplier,
        })
    }
}

impl HashComponent for UseEffects {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        if self.can_sprint != Self::DEFAULT.can_sprint {
            push_hash_entry(&mut entries, "can_sprint", &self.can_sprint);
        }
        if self.interact_vibrations != Self::DEFAULT.interact_vibrations {
            push_hash_entry(
                &mut entries,
                "interact_vibrations",
                &self.interact_vibrations,
            );
        }
        if self.speed_multiplier.to_bits() != Self::DEFAULT.speed_multiplier.to_bits() {
            push_hash_entry(&mut entries, "speed_multiplier", &self.speed_multiplier);
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
    use steel_utils::serial::ReadFrom as _;

    use super::UseEffects;

    fn parse(tag: NbtTag) -> Option<UseEffects> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        UseEffects::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn empty_compound_uses_vanilla_defaults() {
        assert_eq!(
            parse(NbtTag::Compound(NbtCompound::new())),
            Some(UseEffects::DEFAULT)
        );
    }

    #[test]
    fn persistent_codec_rejects_out_of_range_speed() {
        for speed in [-0.1_f32, 1.1, f32::NAN] {
            let mut compound = NbtCompound::new();
            compound.insert("speed_multiplier", speed);
            assert_eq!(parse(NbtTag::Compound(compound)), None);
        }
    }

    #[test]
    fn network_booleans_treat_any_nonzero_byte_as_true() {
        let encoded = [2, 0, 0x3e, 0x4c, 0xcc, 0xcd];
        assert_eq!(
            UseEffects::read(&mut Cursor::new(encoded.as_slice()))
                .expect("use effects should decode"),
            UseEffects::new(true, false, 0.2)
        );
    }
}
