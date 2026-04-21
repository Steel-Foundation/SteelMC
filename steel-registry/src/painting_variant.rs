use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents a painting variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct PaintingVariant {
    pub key: Identifier,
    pub width: i32,
    pub height: i32,
    pub asset_id: Identifier,
    pub title: Option<TextComponent>,
    pub author: Option<TextComponent>,
}

impl ToNbtTag for &PaintingVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("width", self.width);
        compound.insert("height", self.height);
        if let Some(title) = &self.title {
            compound.insert(
                "title",
                NbtTag::Compound(title.to_nbt_tag().into_compound().unwrap()),
            );
        }
        if let Some(author) = &self.author {
            compound.insert(
                "author",
                NbtTag::Compound(author.to_nbt_tag().into_compound().unwrap()),
            );
        }
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(
    PaintingVariantRegistry,
    PaintingVariant,
    stem: painting_variants,
    tagged: "painting variant",
);
