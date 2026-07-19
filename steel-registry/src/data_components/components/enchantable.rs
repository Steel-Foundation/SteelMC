//! Vanilla `minecraft:enchantable` item component.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{Cursor, Error as IoError, Result as IoResult, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidEnchantableValue {
    pub value: i32,
}

impl Display for InvalidEnchantableValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Enchantment value must be positive, but was {}",
            self.value
        )
    }
}

impl Error for InvalidEnchantableValue {}

/// Positive value used to perturb enchanting-table levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enchantable {
    value: i32,
}

impl Enchantable {
    pub const fn new(value: i32) -> Result<Self, InvalidEnchantableValue> {
        if value <= 0 {
            return Err(InvalidEnchantableValue { value });
        }
        Ok(Self { value })
    }

    /// Constructs a value already validated by the extracted-item build step.
    pub(crate) const fn from_extracted_value(value: i32) -> Self {
        assert!(value > 0, "extracted enchantability must be positive");
        Self { value }
    }

    #[must_use]
    pub const fn value(self) -> i32 {
        self.value
    }
}

impl WriteTo for Enchantable {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        VarInt(self.value).write(writer)
    }
}

impl ReadFrom for Enchantable {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Self::new(VarInt::read(data)?.0).map_err(IoError::other)
    }
}

impl ToNbtTag for Enchantable {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("value", self.value);
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for Enchantable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag<'_, '_>) -> Option<Self> {
        let value = tag.compound()?.get("value")?.codec_i32()?;
        Self::new(value).ok()
    }
}

impl HashComponent for Enchantable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.to_nbt_tag().hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::owned::{NbtCompound, NbtTag};
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::Enchantable;

    fn parse(tag: NbtTag) -> Option<Enchantable> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        Enchantable::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn persistent_codec_requires_a_positive_value() {
        for value in [-1, 0] {
            let mut compound = NbtCompound::new();
            compound.insert("value", value);
            assert_eq!(parse(NbtTag::Compound(compound)), None);
        }

        let mut compound = NbtCompound::new();
        compound.insert("value", 15_i8);
        assert_eq!(
            parse(NbtTag::Compound(compound)),
            Some(Enchantable::new(15).expect("15 is positive"))
        );
    }

    #[test]
    fn network_codec_uses_a_validated_varint() {
        let value = Enchantable::new(22).expect("22 is positive");
        let mut encoded = Vec::new();
        value.write(&mut encoded).expect("value should encode");
        assert_eq!(
            Enchantable::read(&mut Cursor::new(encoded.as_slice())).expect("value should decode"),
            value
        );

        let mut zero = Vec::new();
        VarInt(0).write(&mut zero).expect("zero should encode");
        assert!(Enchantable::read(&mut Cursor::new(zero.as_slice())).is_err());
    }

    #[test]
    fn persistent_hash_uses_the_record_codec_shape() {
        let value = Enchantable::new(15).expect("15 is positive");
        assert_eq!(value.compute_hash(), value.to_nbt_tag().compute_hash());
    }
}
