//! Vanilla `minecraft:cooking_fuel` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::resolvable_number::{ResolvableFloat, ResolvableInt};

/// The burn time and cooking-speed multiplier a furnace fuel item provides.
///
/// Literal providers are folded to [`ResolvableInt::Constant`] /
/// [`ResolvableFloat::Constant`] by the build script. Non-literal providers stay
/// as references because Steel has no runtime context-provider registry; they are
/// preserved by the codecs but not evaluated.
#[derive(Debug, Clone, PartialEq)]
pub struct CookingFuel {
    pub burn_time: ResolvableInt,
    pub speed_multiplier: ResolvableFloat,
}

impl CookingFuel {
    #[must_use]
    pub const fn new(burn_time: ResolvableInt, speed_multiplier: ResolvableFloat) -> Self {
        Self {
            burn_time,
            speed_multiplier,
        }
    }
}

impl WriteTo for CookingFuel {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.burn_time.write(writer)?;
        self.speed_multiplier.write(writer)
    }
}

impl ReadFrom for CookingFuel {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            burn_time: ResolvableInt::read(data)?,
            speed_multiplier: ResolvableFloat::read(data)?,
        })
    }
}

impl ToNbtTag for CookingFuel {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("burn_time", self.burn_time.to_nbt_tag());
        compound.insert("speed_multiplier", self.speed_multiplier.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for CookingFuel {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let burn_time = ResolvableInt::from_nbt_tag(compound.get("burn_time")?)?;
        let speed_multiplier = ResolvableFloat::from_nbt_tag(compound.get("speed_multiplier")?)?;
        Some(Self {
            burn_time,
            speed_multiplier,
        })
    }
}

impl HashComponent for CookingFuel {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "burn_time", &self.burn_time);
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

    use super::CookingFuel;
    use crate::resolvable_number::{ResolvableFloat, ResolvableInt};

    fn parse(tag: NbtTag) -> Option<CookingFuel> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        CookingFuel::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn round_trips_references_and_constants() {
        let references = CookingFuel::new(
            ResolvableInt::reference(Identifier::vanilla_static("cooking/time_wood_blocks")),
            ResolvableFloat::reference(Identifier::vanilla_static("cooking/speed_default")),
        );
        assert_eq!(
            parse(references.clone().to_nbt_tag()),
            Some(references.clone())
        );

        let constants =
            CookingFuel::new(ResolvableInt::constant(300), ResolvableFloat::constant(0.5));
        assert_eq!(
            parse(constants.clone().to_nbt_tag()),
            Some(constants.clone())
        );

        for value in [references, constants] {
            let mut network = Vec::new();
            value.write(&mut network).expect("fuel should encode");
            assert_eq!(
                CookingFuel::read(&mut Cursor::new(network.as_slice()))
                    .expect("fuel should decode"),
                value
            );
        }
    }
}
