use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents an armor trim pattern definition from the data packs.
#[derive(Debug)]
pub struct TrimPattern {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub description: TextComponent,
    pub decal: bool,
}

impl ToNbtTag for &TrimPattern {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("description", (&self.description).to_nbt_tag());
        compound.insert("decal", self.decal);
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(TrimPatternRegistry, TrimPattern, stem: trim_patterns);
