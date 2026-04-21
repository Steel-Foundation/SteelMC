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

crate::define_registry!(
    BannerPatternRegistry,
    BannerPattern,
    stem: banner_patterns,
    tagged: "banner pattern",
);
