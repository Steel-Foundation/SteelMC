//! Vanilla `minecraft:compostable` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::serial::{ReadFrom, WriteTo};

/// Reference to the context int provider giving a compostable item its
/// chance to add a compost layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compostable {
    pub layers: Identifier,
}

impl Compostable {
    #[must_use]
    pub const fn new(layers: Identifier) -> Self {
        Self { layers }
    }
}

impl WriteTo for Compostable {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.layers.write(writer)
    }
}

impl ReadFrom for Compostable {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            layers: Identifier::read(data)?,
        })
    }
}

impl ToNbtTag for Compostable {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("layers", self.layers.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for Compostable {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let layers = Identifier::from_nbt_tag(compound.get("layers")?)?;
        Some(Self { layers })
    }
}

impl HashComponent for Compostable {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("layers");
        let mut value_hasher = ComponentHasher::new();
        self.layers.hash_component(&mut value_hasher);
        let entry = HashEntry::new(key_hasher, value_hasher);

        hasher.start_map();
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
        hasher.end_map();
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::FromNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use steel_utils::Identifier;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::Compostable;

    fn parse(tag: NbtTag) -> Option<Compostable> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        Compostable::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn round_trips_both_codecs() {
        let value = Compostable::new(Identifier::vanilla_static("compostable/low"));

        let nbt = value.clone().to_nbt_tag();
        assert_eq!(parse(nbt), Some(value.clone()));

        let mut network = Vec::new();
        value.write(&mut network).expect("compostable should encode");
        assert_eq!(
            Compostable::read(&mut Cursor::new(network.as_slice()))
                .expect("compostable should decode"),
            value
        );
    }
}
