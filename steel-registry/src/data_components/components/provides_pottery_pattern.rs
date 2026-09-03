//! Vanilla `minecraft:provides_pottery_pattern` item component.

use std::io::{Cursor, Error, Result, Write};
use std::str::FromStr;

use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::decorated_pot_pattern::DecoratedPotPatternRef;
use crate::{REGISTRY, RegistryEntry, RegistryExt};

/// Decorated pot pattern supplied by a pottery sherd item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidesPotteryPattern {
    pub pattern: DecoratedPotPatternRef,
}

impl WriteTo for ProvidesPotteryPattern {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let id = self.pattern.try_id().ok_or_else(|| {
            Error::other(format!(
                "Unknown decorated pot pattern: {}",
                self.pattern.key
            ))
        })?;
        let id = i32::try_from(id)
            .map_err(|_| Error::other(format!("Decorated pot pattern id out of range: {id}")))?;
        VarInt(id).write(writer)
    }
}

impl ReadFrom for ProvidesPotteryPattern {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let id = VarInt::read(data)?.0;
        let id = usize::try_from(id)
            .map_err(|_| Error::other(format!("Negative decorated pot pattern id: {id}")))?;
        let pattern = REGISTRY
            .decorated_pot_patterns
            .by_id(id)
            .ok_or_else(|| Error::other(format!("Unknown decorated pot pattern id: {id}")))?;
        Ok(Self { pattern })
    }
}

impl ToNbtTag for ProvidesPotteryPattern {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        self.pattern.key.to_string().to_nbt_tag()
    }
}

impl FromNbtTag for ProvidesPotteryPattern {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let key = Identifier::from_str(&tag.string()?.to_str()).ok()?;
        REGISTRY
            .decorated_pot_patterns
            .by_key(&key)
            .map(|pattern| Self { pattern })
    }
}

impl HashComponent for ProvidesPotteryPattern {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.put_string(&self.pattern.key.to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::ProvidesPotteryPattern;
    use crate::data_components::vanilla_components::PROVIDES_POTTERY_PATTERN;
    use crate::init_vanilla_registry;
    use crate::item_stack::ItemStack;
    use crate::{REGISTRY, vanilla_decorated_pot_patterns, vanilla_items};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<ProvidesPotteryPattern> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ProvidesPotteryPattern::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn round_trips_both_codecs_and_hashes_by_key() {
        init_vanilla_registry();
        let component = ProvidesPotteryPattern {
            pattern: &vanilla_decorated_pot_patterns::ANGLER,
        };

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("pottery pattern should encode");
        assert_eq!(
            ProvidesPotteryPattern::read(&mut Cursor::new(network.as_slice()))
                .expect("pottery pattern should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(
            nbt,
            simdnbt::owned::NbtTag::String("minecraft:angler".into())
        );
        assert_eq!(parse(nbt), Some(component));
    }

    #[test]
    fn extracted_sherd_item_provides_its_pattern() {
        init_vanilla_registry();
        let component = ItemStack::new(&vanilla_items::ANGLER_POTTERY_SHERD)
            .get(PROVIDES_POTTERY_PATTERN)
            .expect("angler pottery sherd should provide a pattern");
        assert_eq!(component.pattern.key, vanilla_decorated_pot_patterns::ANGLER.key);
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(PROVIDES_POTTERY_PATTERN))
                .count(),
            23
        );
    }
}
