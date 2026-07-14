//! Armor trim pattern registry values.

use std::io::{Cursor, Result, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::nbt::NbtNumeric as _;
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};

/// Complete registry-independent trim pattern definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimPatternValue {
    asset_id: Identifier,
    description: TextComponent,
    decal: bool,
}

impl TrimPatternValue {
    #[must_use]
    pub const fn new(asset_id: Identifier, description: TextComponent, decal: bool) -> Self {
        Self {
            asset_id,
            description,
            decal,
        }
    }

    #[must_use]
    pub const fn asset_id(&self) -> &Identifier {
        &self.asset_id
    }

    #[must_use]
    pub const fn description(&self) -> &TextComponent {
        &self.description
    }

    #[must_use]
    pub const fn decal(&self) -> bool {
        self.decal
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("description", self.description.to_codec_nbt());
        compound.insert("decal", self.decal);
        NbtTag::Compound(compound)
    }
}

impl WriteTo for TrimPatternValue {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.asset_id.write(writer)?;
        WriteTo::write(&self.description.to_codec_nbt(), writer)?;
        self.decal.write(writer)
    }
}

impl ReadFrom for TrimPatternValue {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            Identifier::read(data)?,
            TextComponent::read(data)?,
            bool::read(data)?,
        ))
    }
}

impl ToNbtTag for TrimPatternValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for TrimPatternValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            Identifier::from_nbt_tag(compound.get("asset_id")?)?,
            TextComponent::from_nbt(&compound.get("description")?.to_owned())?,
            compound
                .get("decal")
                .map_or(Some(false), |decal| decal.codec_bool())?,
        ))
    }
}

impl HashComponent for TrimPatternValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "asset_id", &self.asset_id);
        push_hash_entry(&mut entries, "description", &self.description);
        push_hash_entry(&mut entries, "decal", &self.decal);
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

/// Registered armor trim pattern definition.
#[derive(Debug)]
pub struct TrimPattern {
    pub key: Identifier,
    value: TrimPatternValue,
}

impl TrimPattern {
    #[must_use]
    pub const fn new(key: Identifier, value: TrimPatternValue) -> Self {
        Self { key, value }
    }

    #[must_use]
    pub const fn value(&self) -> &TrimPatternValue {
        &self.value
    }
}

impl ToNbtTag for &TrimPattern {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.to_nbt_tag_ref()
    }
}

pub type TrimPatternRef = &'static TrimPattern;

pub struct TrimPatternRegistry {
    trim_patterns_by_id: Vec<TrimPatternRef>,
    trim_patterns_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl TrimPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            trim_patterns_by_id: Vec::new(),
            trim_patterns_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    TrimPatternRegistry,
    TrimPatternRef,
    trim_patterns_by_id,
    trim_patterns_by_key,
    allows_registering
);

crate::impl_registry!(
    TrimPatternRegistry,
    TrimPattern,
    trim_patterns_by_id,
    trim_patterns_by_key,
    trim_patterns
);
crate::impl_tagged_registry!(TrimPatternRegistry, trim_patterns_by_key, "trim pattern");

impl RegistryHolderEntry for TrimPattern {
    type Value = TrimPatternValue;

    const REGISTRY_NAME: &'static str = "trim pattern";

    fn holder_value(&self) -> &Self::Value {
        &self.value
    }

    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.trim_patterns.by_id(id)
    }

    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.trim_patterns.by_key(key)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;

    use super::TrimPatternValue;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, vanilla_trim_patterns};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<TrimPatternValue> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        TrimPatternValue::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn generated_patterns_follow_vanilla_registry_order() {
        init_test_registry();
        let keys = REGISTRY
            .trim_patterns
            .iter()
            .map(|(_, pattern)| pattern.key.path.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "sentry",
                "dune",
                "coast",
                "wild",
                "ward",
                "eye",
                "vex",
                "tide",
                "snout",
                "rib",
                "spire",
                "wayfinder",
                "shaper",
                "silence",
                "raiser",
                "host",
                "flow",
                "bolt",
            ]
        );
    }

    #[test]
    fn direct_codecs_always_encode_decal_and_default_it_when_absent() {
        init_test_registry();
        let pattern = vanilla_trim_patterns::SENTRY.value().clone();

        let mut network = Vec::new();
        pattern.write(&mut network).expect("pattern should encode");
        assert_eq!(
            TrimPatternValue::read(&mut Cursor::new(network.as_slice()))
                .expect("pattern should decode"),
            pattern
        );

        let nbt = pattern.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(pattern.clone()));
        // HashOps preserves Codec.BOOL while NbtOps represents it as a byte.
        assert_ne!(pattern.compute_hash(), nbt.compute_hash());
        let simdnbt::owned::NbtTag::Compound(mut compound) = nbt else {
            panic!("pattern should encode as a compound");
        };
        assert_eq!(compound.byte("decal"), Some(0));
        compound.remove("decal");
        assert_eq!(
            parse(simdnbt::owned::NbtTag::Compound(compound)),
            Some(pattern)
        );
    }

    #[test]
    fn direct_persistent_codec_rejects_invalid_asset_identifiers() {
        let mut compound = simdnbt::owned::NbtCompound::new();
        compound.insert("asset_id", "Invalid Asset");
        compound.insert("description", TextComponent::plain("Invalid").to_nbt_tag());
        compound.insert("decal", false);
        assert!(parse(simdnbt::owned::NbtTag::Compound(compound)).is_none());

        assert_eq!(
            vanilla_trim_patterns::SENTRY.value().asset_id(),
            &Identifier::vanilla_static("sentry")
        );
    }
}
