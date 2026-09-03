//! Decorated pot pattern registry values.

use std::io::{Cursor, Result as IoResult, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry};
use steel_utils::serial::{ReadFrom, WriteTo};

/// Complete registry-independent decorated pot pattern definition.
///
/// Mirrors vanilla's `DecoratedPotPattern(ResourceLocation assetId)` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecoratedPotPatternValue {
    asset_id: Identifier,
}

impl DecoratedPotPatternValue {
    #[must_use]
    pub const fn new(asset_id: Identifier) -> Self {
        Self { asset_id }
    }

    #[must_use]
    pub const fn asset_id(&self) -> &Identifier {
        &self.asset_id
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        NbtTag::Compound(compound)
    }
}

impl WriteTo for DecoratedPotPatternValue {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.asset_id.write(writer)
    }
}

impl ReadFrom for DecoratedPotPatternValue {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Ok(Self::new(Identifier::read(data)?))
    }
}

impl ToNbtTag for DecoratedPotPatternValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for DecoratedPotPatternValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let asset_id = Identifier::from_nbt_tag(compound.get("asset_id")?)?;
        Some(Self::new(asset_id))
    }
}

impl HashComponent for DecoratedPotPatternValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut key_hasher = ComponentHasher::new();
        key_hasher.put_string("asset_id");
        let mut value_hasher = ComponentHasher::new();
        self.asset_id.hash_component(&mut value_hasher);
        let entry = HashEntry::new(key_hasher, value_hasher);

        hasher.start_map();
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
        hasher.end_map();
    }
}

/// Registered decorated pot pattern definition.
#[derive(Debug)]
pub struct DecoratedPotPattern {
    pub key: Identifier,
    value: DecoratedPotPatternValue,
}

impl DecoratedPotPattern {
    #[must_use]
    pub const fn new(key: Identifier, value: DecoratedPotPatternValue) -> Self {
        Self { key, value }
    }

    #[must_use]
    pub const fn value(&self) -> &DecoratedPotPatternValue {
        &self.value
    }
}

impl ToNbtTag for &DecoratedPotPattern {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.to_nbt_tag_ref()
    }
}

pub type DecoratedPotPatternRef = &'static DecoratedPotPattern;

pub struct DecoratedPotPatternRegistry {
    patterns_by_id: Vec<DecoratedPotPatternRef>,
    patterns_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl DecoratedPotPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            patterns_by_id: Vec::new(),
            patterns_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    DecoratedPotPatternRegistry,
    DecoratedPotPatternRef,
    patterns_by_id,
    patterns_by_key,
    allows_registering
);

crate::impl_registry!(
    DecoratedPotPatternRegistry,
    DecoratedPotPattern,
    patterns_by_id,
    patterns_by_key,
    decorated_pot_patterns
);

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::DecoratedPotPatternValue;
    use crate::init_vanilla_registry;
    use crate::{REGISTRY, vanilla_decorated_pot_patterns};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<DecoratedPotPatternValue> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        DecoratedPotPatternValue::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn generated_patterns_are_registered() {
        init_vanilla_registry();
        assert!(
            REGISTRY
                .decorated_pot_patterns
                .iter()
                .any(|(_, pattern)| pattern.key.path.as_ref() == "angler")
        );
    }

    #[test]
    fn direct_codecs_and_hash_match_vanilla_shape() {
        init_vanilla_registry();
        let pattern = vanilla_decorated_pot_patterns::ANGLER.value().clone();
        let mut network = Vec::new();
        pattern.write(&mut network).expect("pattern should encode");
        assert_eq!(
            DecoratedPotPatternValue::read(&mut Cursor::new(network.as_slice()))
                .expect("pattern should decode"),
            pattern
        );

        let nbt = pattern.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(pattern.clone()));
        assert_eq!(pattern.compute_hash(), nbt.compute_hash());
    }
}
