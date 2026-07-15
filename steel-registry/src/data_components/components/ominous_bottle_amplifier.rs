//! Vanilla `minecraft:ominous_bottle_amplifier` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::NbtTag;
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OminousBottleAmplifier {
    value: i32,
}

impl OminousBottleAmplifier {
    pub const EFFECT_DURATION: i32 = 120_000;
    pub const MIN_AMPLIFIER: i32 = 0;
    pub const MAX_AMPLIFIER: i32 = 4;

    #[must_use]
    pub const fn new(value: i32) -> Self {
        Self { value }
    }

    #[must_use]
    pub const fn value(self) -> i32 {
        self.value
    }
}

impl WriteTo for OminousBottleAmplifier {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.value).write(writer)
    }
}

impl ReadFrom for OminousBottleAmplifier {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(VarInt::read(data)?.0))
    }
}

impl ToNbtTag for OminousBottleAmplifier {
    fn to_nbt_tag(self) -> NbtTag {
        NbtTag::Int(self.value)
    }
}

impl FromNbtTag for OminousBottleAmplifier {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let value = tag.codec_i32()?;
        (Self::MIN_AMPLIFIER..=Self::MAX_AMPLIFIER)
            .contains(&value)
            .then(|| Self::new(value))
    }
}

impl HashComponent for OminousBottleAmplifier {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_int(self.value);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::OminousBottleAmplifier;

    fn parse(tag: NbtTag) -> Option<OminousBottleAmplifier> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        OminousBottleAmplifier::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn persistent_codec_enforces_vanilla_range() {
        assert_eq!(parse(NbtTag::Byte(0)), Some(OminousBottleAmplifier::new(0)));
        assert_eq!(parse(NbtTag::Long(4)), Some(OminousBottleAmplifier::new(4)));
        assert_eq!(parse(NbtTag::Int(-1)), None);
        assert_eq!(parse(NbtTag::Int(5)), None);
    }

    #[test]
    fn network_codec_accepts_any_varint_like_vanilla_constructor() {
        for value in [-1, 0, 4, 5, i32::MAX] {
            let mut encoded = Vec::new();
            VarInt(value)
                .write(&mut encoded)
                .expect("amplifier should encode");
            assert_eq!(
                OminousBottleAmplifier::read(&mut Cursor::new(encoded.as_slice()))
                    .expect("amplifier should decode"),
                OminousBottleAmplifier::new(value)
            );
        }
    }
}
