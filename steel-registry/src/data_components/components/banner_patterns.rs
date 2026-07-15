//! Vanilla `minecraft:banner_patterns` item component.

use std::io::{Cursor, Error, Result, Write};

use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::banner_pattern::BannerPattern;
use crate::{DyeColor, RegistryHolder};

/// One pattern and color layer on a banner or shield.
#[derive(Debug, Clone, PartialEq)]
pub struct BannerPatternLayer {
    pattern: RegistryHolder<BannerPattern>,
    color: DyeColor,
}

impl BannerPatternLayer {
    #[must_use]
    pub const fn new(pattern: RegistryHolder<BannerPattern>, color: DyeColor) -> Self {
        Self { pattern, color }
    }

    #[must_use]
    pub const fn pattern(&self) -> &RegistryHolder<BannerPattern> {
        &self.pattern
    }

    #[must_use]
    pub const fn color(&self) -> DyeColor {
        self.color
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("pattern", self.pattern.clone().to_nbt_tag());
        compound.insert("color", self.color.to_nbt_tag());
        NbtTag::Compound(compound)
    }

    fn from_nbt_compound(compound: simdnbt::borrow::NbtCompound<'_, '_>) -> Option<Self> {
        Some(Self::new(
            RegistryHolder::from_nbt_tag(compound.get("pattern")?)?,
            DyeColor::from_nbt_tag(compound.get("color")?)?,
        ))
    }
}

impl WriteTo for BannerPatternLayer {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.pattern.write(writer)?;
        self.color.write(writer)
    }
}

impl ReadFrom for BannerPatternLayer {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            RegistryHolder::read(data)?,
            DyeColor::read(data)?,
        ))
    }
}

impl ToNbtTag for BannerPatternLayer {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for BannerPatternLayer {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        Self::from_nbt_compound(tag.compound()?)
    }
}

impl HashComponent for BannerPatternLayer {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "pattern", &self.pattern);
        push_hash_entry(&mut entries, "color", &self.color);
        sort_map_entries(&mut entries);
        hasher.start_map();
        for entry in &entries {
            hasher.put_raw_bytes(&entry.key_bytes);
            hasher.put_raw_bytes(&entry.value_bytes);
        }
        hasher.end_map();
    }
}

/// Ordered banner pattern layers.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct BannerPatternLayers {
    layers: Vec<BannerPatternLayer>,
}

impl BannerPatternLayers {
    #[must_use]
    pub const fn empty() -> Self {
        Self { layers: Vec::new() }
    }

    #[must_use]
    pub const fn new(layers: Vec<BannerPatternLayer>) -> Self {
        Self { layers }
    }

    #[must_use]
    pub fn layers(&self) -> &[BannerPatternLayer] {
        &self.layers
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        if self.layers.is_empty() {
            return NbtTag::List(NbtList::Empty);
        }
        NbtTag::List(NbtList::Compound(
            self.layers
                .iter()
                .map(|layer| match layer.to_nbt_tag_ref() {
                    NbtTag::Compound(compound) => compound,
                    _ => unreachable!("banner layer codec always produces a compound"),
                })
                .collect(),
        ))
    }
}

impl WriteTo for BannerPatternLayers {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        let count = i32::try_from(self.layers.len())
            .map_err(|_| Error::other("Too many banner pattern layers"))?;
        VarInt(count).write(writer)?;
        for layer in &self.layers {
            layer.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for BannerPatternLayers {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let count = VarInt::read(data)?.0;
        let count = usize::try_from(count)
            .map_err(|_| Error::other("Negative banner pattern layer count"))?;
        let mut layers = Vec::with_capacity(count.min(65_536));
        for _ in 0..count {
            layers.push(BannerPatternLayer::read(data)?);
        }
        Ok(Self::new(layers))
    }
}

impl ToNbtTag for BannerPatternLayers {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for BannerPatternLayers {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let list = tag.list()?;
        if list.to_owned().as_nbt_tags().is_empty() {
            return Some(Self::empty());
        }
        let layers = list
            .compounds()?
            .into_iter()
            .map(BannerPatternLayer::from_nbt_compound)
            .collect::<Option<Vec<_>>>()?;
        Some(Self::new(layers))
    }
}

impl HashComponent for BannerPatternLayers {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        hasher.start_list();
        for layer in &self.layers {
            hasher.put_component_hash(layer);
        }
        hasher.end_list();
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
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

    use super::{BannerPatternLayer, BannerPatternLayers};
    use crate::banner_pattern::BannerPatternValue;
    use crate::data_components::vanilla_components::BANNER_PATTERNS;
    use crate::test_support::init_test_registry;
    use crate::{DyeColor, REGISTRY, RegistryHolder, vanilla_banner_patterns};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<BannerPatternLayers> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        BannerPatternLayers::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn layer_references_round_trip_both_codecs_and_hash_the_list() {
        init_test_registry();
        let layers = BannerPatternLayers::new(vec![BannerPatternLayer::new(
            RegistryHolder::reference(&vanilla_banner_patterns::CREEPER),
            DyeColor::Lime,
        )]);
        let nbt = layers.clone().to_nbt_tag();
        assert_eq!(parse(nbt), Some(layers.clone()));

        let mut network = Vec::new();
        layers.write(&mut network).expect("layers should encode");
        let decoded = BannerPatternLayers::read(&mut Cursor::new(network.as_slice()))
            .expect("layers should decode");
        assert_eq!(decoded, layers);
        assert_eq!(decoded.compute_hash(), layers.compute_hash());
    }

    #[test]
    fn inline_patterns_round_trip_both_holder_codecs() {
        init_test_registry();
        let direct = BannerPatternValue::new(
            Identifier::new_static("steel", "wave"),
            Cow::Borrowed("block.steel.banner.wave"),
        );
        let layers = BannerPatternLayers::new(vec![BannerPatternLayer::new(
            RegistryHolder::direct(direct),
            DyeColor::Blue,
        )]);
        assert_eq!(parse(layers.clone().to_nbt_tag()), Some(layers.clone()));
        let mut network = Vec::new();
        layers.write(&mut network).expect("layers should encode");
        assert_eq!(
            BannerPatternLayers::read(&mut Cursor::new(network.as_slice()))
                .expect("layers should decode"),
            layers
        );
    }

    #[test]
    fn empty_layers_use_vanilla_empty_list_shape() {
        let layers = BannerPatternLayers::empty();
        assert_eq!(parse(layers.clone().to_nbt_tag()), Some(layers.clone()));
        let mut network = Vec::new();
        layers.write(&mut network).expect("layers should encode");
        assert_eq!(network, [0]);
    }

    #[test]
    fn extracted_banners_and_shield_keep_empty_layers() {
        init_test_registry();
        let items = REGISTRY
            .items
            .iter()
            .filter(|(_, item)| item.components.has(BANNER_PATTERNS))
            .collect::<Vec<_>>();
        assert_eq!(items.len(), 17);
        assert!(items.iter().all(|(_, item)| {
            item.components.get(BANNER_PATTERNS) == Some(BannerPatternLayers::empty())
        }));
    }
}
