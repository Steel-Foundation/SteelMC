//! Vanilla `minecraft:brewing_fuel` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::resolvable_number::{ResolvableFloat, ResolvableInt};

/// The number of uses and brewing-speed multiplier a fuel item provides.
///
/// Literal providers are folded to [`ResolvableInt::Constant`] /
/// [`ResolvableFloat::Constant`] by the build script. Non-literal providers stay
/// as references because Steel has no runtime context-provider registry; they are
/// preserved by the codecs but not evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct BrewingFuel {
    pub uses: ResolvableInt,
    pub speed_multiplier: ResolvableFloat,
}

impl BrewingFuel {
    #[must_use]
    pub const fn new(uses: ResolvableInt, speed_multiplier: ResolvableFloat) -> Self {
        Self {
            uses,
            speed_multiplier,
        }
    }
}

impl WriteTo for BrewingFuel {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.uses.write(writer)?;
        self.speed_multiplier.write(writer)
    }
}

impl ReadFrom for BrewingFuel {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            uses: ResolvableInt::read(data)?,
            speed_multiplier: ResolvableFloat::read(data)?,
        })
    }
}

impl ToNbtTag for BrewingFuel {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("uses", self.uses.to_nbt_tag());
        compound.insert("speed_multiplier", self.speed_multiplier.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for BrewingFuel {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let uses = ResolvableInt::from_nbt_tag(compound.get("uses")?)?;
        let speed_multiplier = ResolvableFloat::from_nbt_tag(compound.get("speed_multiplier")?)?;
        Some(Self {
            uses,
            speed_multiplier,
        })
    }
}

impl HashComponent for BrewingFuel {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "uses", &self.uses);
        push_hash_entry(&mut entries, "speed_multiplier", &self.speed_multiplier);
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

    use simdnbt::FromNbtTag as _;
    use simdnbt::ToNbtTag as _;
    use simdnbt::borrow::read_tag;
    use simdnbt::owned::NbtTag;
    use steel_utils::Identifier;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::BrewingFuel;
    use crate::resolvable_number::{ResolvableFloat, ResolvableInt};

    fn parse(tag: NbtTag) -> Option<BrewingFuel> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        BrewingFuel::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn round_trips_references_and_constants() {
        let references = BrewingFuel::new(
            ResolvableInt::reference(Identifier::vanilla_static("brewing/uses_default")),
            ResolvableFloat::reference(Identifier::vanilla_static("brewing/speed_default")),
        );
        assert_eq!(
            parse(references.clone().to_nbt_tag()),
            Some(references.clone())
        );

        let constants =
            BrewingFuel::new(ResolvableInt::constant(20), ResolvableFloat::constant(1.5));
        assert_eq!(
            parse(constants.clone().to_nbt_tag()),
            Some(constants.clone())
        );

        for value in [references, constants] {
            let mut network = Vec::new();
            value.write(&mut network).expect("fuel should encode");
            assert_eq!(
                BrewingFuel::read(&mut Cursor::new(network.as_slice()))
                    .expect("fuel should decode"),
                value
            );
        }
    }
}
