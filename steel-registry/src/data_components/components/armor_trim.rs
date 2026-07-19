//! Vanilla `minecraft:trim` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolder;
use crate::trim_material::TrimMaterial;
use crate::trim_pattern::TrimPattern;

/// Material and pattern applied to a trimmed equipment item.
#[derive(Debug, Clone, PartialEq)]
pub struct ArmorTrim {
    material: RegistryHolder<TrimMaterial>,
    pattern: RegistryHolder<TrimPattern>,
}

impl ArmorTrim {
    #[must_use]
    pub const fn new(
        material: RegistryHolder<TrimMaterial>,
        pattern: RegistryHolder<TrimPattern>,
    ) -> Self {
        Self { material, pattern }
    }

    #[must_use]
    pub const fn material(&self) -> &RegistryHolder<TrimMaterial> {
        &self.material
    }

    #[must_use]
    pub const fn pattern(&self) -> &RegistryHolder<TrimPattern> {
        &self.pattern
    }
}

impl WriteTo for ArmorTrim {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.material.write(writer)?;
        self.pattern.write(writer)
    }
}

impl ReadFrom for ArmorTrim {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self::new(
            RegistryHolder::read(data)?,
            RegistryHolder::read(data)?,
        ))
    }
}

impl ToNbtTag for ArmorTrim {
    fn to_nbt_tag(self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("material", self.material.to_nbt_tag());
        compound.insert("pattern", self.pattern.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

impl FromNbtTag for ArmorTrim {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        Some(Self::new(
            RegistryHolder::from_nbt_tag(compound.get("material")?)?,
            RegistryHolder::from_nbt_tag(compound.get("pattern")?)?,
        ))
    }
}

impl HashComponent for ArmorTrim {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        push_hash_entry(&mut entries, "material", &self.material);
        push_hash_entry(&mut entries, "pattern", &self.pattern);
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

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rustc_hash::FxHashMap;
    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{ReadFrom as _, WriteTo as _};
    use text_components::TextComponent;

    use super::ArmorTrim;
    use crate::RegistryHolder;
    use crate::data_components::vanilla_components::TRIM;
    use crate::test_support::init_test_registry;
    use crate::trim_material::{MaterialAssetGroup, MaterialAssetInfo, TrimMaterialValue};
    use crate::trim_pattern::TrimPatternValue;
    use crate::{REGISTRY, vanilla_trim_materials, vanilla_trim_patterns};

    fn parse(tag: simdnbt::owned::NbtTag) -> Option<ArmorTrim> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ArmorTrim::from_nbt_tag(borrowed.as_tag())
    }

    #[test]
    fn registry_references_round_trip_both_codecs_and_hash_the_record() {
        init_test_registry();
        let trim = ArmorTrim::new(
            RegistryHolder::reference(&vanilla_trim_materials::IRON),
            RegistryHolder::reference(&vanilla_trim_patterns::SENTRY),
        );

        let mut network = Vec::new();
        trim.write(&mut network).expect("trim should encode");
        assert_eq!(
            ArmorTrim::read(&mut Cursor::new(network.as_slice())).expect("trim should decode"),
            trim
        );

        let nbt = trim.clone().to_nbt_tag();
        assert_eq!(parse(nbt.clone()), Some(trim.clone()));
        assert_eq!(trim.compute_hash(), nbt.compute_hash());
        let simdnbt::owned::NbtTag::Compound(compound) = nbt else {
            panic!("trim should encode as a compound");
        };
        assert_eq!(
            compound
                .string("material")
                .map(|value| value.to_str().into_owned()),
            Some("minecraft:iron".to_owned())
        );
        assert_eq!(
            compound
                .string("pattern")
                .map(|value| value.to_str().into_owned()),
            Some("minecraft:sentry".to_owned())
        );
    }

    #[test]
    fn inline_material_and_pattern_round_trip_both_codecs() {
        init_test_registry();
        let trim = ArmorTrim::new(
            RegistryHolder::direct(TrimMaterialValue::new(
                MaterialAssetGroup::new(
                    MaterialAssetInfo::new("custom").expect("test suffix should be valid"),
                    FxHashMap::default(),
                ),
                TextComponent::plain("Custom material"),
            )),
            RegistryHolder::direct(TrimPatternValue::new(
                Identifier::vanilla_static("custom"),
                TextComponent::plain("Custom pattern"),
                true,
            )),
        );

        let mut network = Vec::new();
        trim.write(&mut network).expect("inline trim should encode");
        let decoded = ArmorTrim::read(&mut Cursor::new(network.as_slice()))
            .expect("inline trim should decode");
        assert_eq!(decoded, trim);
        assert_eq!(decoded.compute_hash(), trim.compute_hash());
        let nbt = trim.clone().to_nbt_tag();
        let parsed = parse(nbt).expect("inline trim NBT should decode");
        assert_eq!(parsed, trim);
        assert_eq!(parsed.compute_hash(), trim.compute_hash());
    }

    #[test]
    fn extracted_item_prototypes_do_not_define_a_default_trim() {
        init_test_registry();
        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(TRIM))
                .count(),
            0
        );
    }
}
