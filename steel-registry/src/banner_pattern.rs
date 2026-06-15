use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a banner pattern definition from a data pack JSON file.
#[derive(Debug)]
pub struct BannerPattern {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub translation_key: &'static str,
}

impl ToNbtTag for &BannerPattern {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("translation_key", self.translation_key);
        NbtTag::Compound(compound)
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
