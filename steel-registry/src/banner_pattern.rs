//! Banner pattern registry values.

use std::borrow::Cow;
use std::io::{Cursor, Error, Result, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};

use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};

const MAX_NETWORK_STRING_LENGTH: usize = 32_767;
const MAX_NETWORK_STRING_BYTES: usize = MAX_NETWORK_STRING_LENGTH * 3;

/// Complete registry-independent banner pattern definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerPatternValue {
    asset_id: Identifier,
    translation_key: Cow<'static, str>,
}

impl BannerPatternValue {
    #[must_use]
    pub const fn new(asset_id: Identifier, translation_key: Cow<'static, str>) -> Self {
        Self {
            asset_id,
            translation_key,
        }
    }

    #[must_use]
    pub const fn asset_id(&self) -> &Identifier {
        &self.asset_id
    }

    #[must_use]
    pub fn translation_key(&self) -> &str {
        &self.translation_key
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("translation_key", self.translation_key.as_ref());
        NbtTag::Compound(compound)
    }
}

impl WriteTo for BannerPatternValue {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.asset_id.write(writer)?;
        write_network_string(&self.translation_key, writer)
    }
}

impl ReadFrom for BannerPatternValue {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            Identifier::read(data)?,
            Cow::Owned(read_network_string(data)?),
        ))
    }
}

impl ToNbtTag for BannerPatternValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for BannerPatternValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let asset_id = Identifier::from_nbt_tag(compound.get("asset_id")?)?;
        let translation_key = compound.get("translation_key")?.string()?.to_str();
        Some(Self::new(
            asset_id,
            Cow::Owned(translation_key.into_owned()),
        ))
    }
}

impl HashComponent for BannerPatternValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "asset_id", &self.asset_id);
        push_hash_entry(
            &mut entries,
            "translation_key",
            self.translation_key.as_ref(),
        );
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

fn write_network_string(value: &str, writer: &mut impl Write) -> Result<()> {
    if value.encode_utf16().count() > MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("String is longer than 32767 UTF-16 units"));
    }
    if value.len() > MAX_NETWORK_STRING_BYTES {
        return Err(Error::other("Encoded string is longer than 98301 bytes"));
    }
    value.write_prefixed::<VarInt>(writer)
}

fn read_network_string(data: &mut Cursor<&[u8]>) -> Result<String> {
    let value = String::read_prefixed_bound::<VarInt>(data, MAX_NETWORK_STRING_BYTES)?;
    if value.encode_utf16().count() > MAX_NETWORK_STRING_LENGTH {
        return Err(Error::other("String is longer than 32767 UTF-16 units"));
    }
    Ok(value)
}

/// Registered banner pattern definition.
#[derive(Debug)]
pub struct BannerPattern {
    pub key: Identifier,
    value: BannerPatternValue,
}

impl BannerPattern {
    #[must_use]
    pub const fn new(key: Identifier, value: BannerPatternValue) -> Self {
        Self { key, value }
    }

    #[must_use]
    pub const fn value(&self) -> &BannerPatternValue {
        &self.value
    }
}

impl ToNbtTag for &BannerPattern {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.to_nbt_tag_ref()
    }
}

pub type BannerPatternRef = &'static BannerPattern;

pub struct BannerPatternRegistry {
    banner_patterns_by_id: Vec<BannerPatternRef>,
    banner_patterns_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl BannerPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            banner_patterns_by_id: Vec::new(),
            banner_patterns_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    BannerPatternRegistry,
    BannerPatternRef,
    banner_patterns_by_id,
    banner_patterns_by_key,
    allows_registering
);

crate::impl_registry!(
    BannerPatternRegistry,
    BannerPattern,
    banner_patterns_by_id,
    banner_patterns_by_key,
    banner_patterns
);

crate::impl_tagged_registry!(
    BannerPatternRegistry,
    banner_patterns_by_key,
    "banner pattern"
);

impl RegistryHolderEntry for BannerPattern {
    type Value = BannerPatternValue;

    const REGISTRY_NAME: &'static str = "banner pattern";

    fn holder_value(&self) -> &Self::Value {
        &self.value
    }

    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.banner_patterns.by_id(id)
    }

    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.banner_patterns.by_key(key)
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::io::Cursor;

    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};

    use super::BannerPatternValue;
    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, vanilla_banner_patterns};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<BannerPatternValue> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        BannerPatternValue::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn generated_patterns_follow_vanilla_registry_order() {
        init_test_registry();
        let keys = REGISTRY
            .banner_patterns
            .iter()
            .map(|(_, pattern)| pattern.key.path.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "base",
                "square_bottom_left",
                "square_bottom_right",
                "square_top_left",
                "square_top_right",
                "stripe_bottom",
                "stripe_top",
                "stripe_left",
                "stripe_right",
                "stripe_center",
                "stripe_middle",
                "stripe_downright",
                "stripe_downleft",
                "small_stripes",
                "cross",
                "straight_cross",
                "triangle_bottom",
                "triangle_top",
                "triangles_bottom",
                "triangles_top",
                "diagonal_left",
                "diagonal_up_right",
                "diagonal_up_left",
                "diagonal_right",
                "circle",
                "rhombus",
                "half_vertical",
                "half_horizontal",
                "half_vertical_right",
                "half_horizontal_bottom",
                "border",
                "gradient",
                "gradient_up",
                "bricks",
                "curly_border",
                "globe",
                "creeper",
                "skull",
                "flower",
                "mojang",
                "piglin",
                "flow",
                "guster",
            ]
        );
    }

    #[test]
    fn direct_codecs_and_hash_match_vanilla_shape() {
        init_test_registry();
        let pattern = vanilla_banner_patterns::BASE.value().clone();
        let mut network = Vec::new();
        pattern.write(&mut network).expect("pattern should encode");
        assert_eq!(
            BannerPatternValue::read(&mut Cursor::new(network.as_slice()))
                .expect("pattern should decode"),
            pattern
        );

        let nbt = pattern.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(pattern.clone()));
        assert_eq!(pattern.compute_hash(), nbt.compute_hash());
    }

    #[test]
    fn direct_codecs_reject_invalid_identifiers_and_long_network_strings() {
        let mut compound = simdnbt::owned::NbtCompound::new();
        compound.insert("asset_id", "Invalid Asset");
        compound.insert("translation_key", "block.minecraft.banner.invalid");
        assert!(parse(simdnbt::owned::NbtTag::Compound(compound)).is_none());

        let too_long = BannerPatternValue::new(
            Identifier::vanilla_static("base"),
            Cow::Owned("x".repeat(32_768)),
        );
        assert!(too_long.write(&mut Vec::new()).is_err());
    }
}
