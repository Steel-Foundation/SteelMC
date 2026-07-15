//! Vanilla `minecraft:provides_trim_material` item component.

use std::io::{Cursor, Result, Write};

use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::hash::{ComponentHasher, HashComponent};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::RegistryHolder;
use crate::trim_material::{TrimMaterial, TrimMaterialValue};

/// Trim material supplied by an ingredient in the smithing table.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvidesTrimMaterial {
    material: RegistryHolder<TrimMaterial>,
}

impl ProvidesTrimMaterial {
    #[must_use]
    pub const fn new(material: RegistryHolder<TrimMaterial>) -> Self {
        Self { material }
    }

    #[must_use]
    pub const fn material(&self) -> &RegistryHolder<TrimMaterial> {
        &self.material
    }

    #[must_use]
    pub fn value(&self) -> &TrimMaterialValue {
        self.material.value()
    }
}

impl WriteTo for ProvidesTrimMaterial {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.material.write(writer)
    }
}

impl ReadFrom for ProvidesTrimMaterial {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        RegistryHolder::read(data).map(Self::new)
    }
}

impl ToNbtTag for ProvidesTrimMaterial {
    fn to_nbt_tag(self) -> simdnbt::owned::NbtTag {
        self.material.to_nbt_tag()
    }
}

impl FromNbtTag for ProvidesTrimMaterial {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        RegistryHolder::from_nbt_tag(tag).map(Self::new)
    }
}

impl HashComponent for ProvidesTrimMaterial {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.material.hash_component(hasher);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use rustc_hash::FxHashMap;
    use simdnbt::borrow::read_tag;
    use simdnbt::{FromNbtTag as _, ToNbtTag as _};
    use steel_utils::Identifier;
    use steel_utils::codec::VarInt;
    use steel_utils::hash::HashComponent as _;
    use steel_utils::serial::{PrefixedWrite as _, ReadFrom as _, WriteTo as _};
    use text_components::{TextComponent, format::Color};

    use super::ProvidesTrimMaterial;
    use crate::RegistryHolder;
    use crate::data_components::vanilla_components::PROVIDES_TRIM_MATERIAL;
    use crate::item_stack::ItemStack;
    use crate::test_support::init_test_registry;
    use crate::trim_material::{MaterialAssetGroup, MaterialAssetInfo, TrimMaterialValue};
    use crate::{REGISTRY, vanilla_items, vanilla_trim_materials};

    fn parse_component(tag: simdnbt::owned::NbtTag) -> Option<ProvidesTrimMaterial> {
        let mut bytes = Vec::new();
        tag.write(&mut bytes);
        let borrowed = read_tag(&mut Cursor::new(bytes.as_slice())).ok()?;
        ProvidesTrimMaterial::from_nbt_tag(borrowed.as_tag())
    }

    fn inline_component() -> ProvidesTrimMaterial {
        let mut description = TextComponent::plain("Custom trim material");
        description.format.color = Some(Color::Rgb(0x12, 0x34, 0x56));
        let overrides = FxHashMap::from_iter([(
            Identifier::vanilla_static("iron"),
            MaterialAssetInfo::new("custom_darker").expect("test suffix should be valid"),
        )]);
        ProvidesTrimMaterial::new(RegistryHolder::direct(TrimMaterialValue::new(
            MaterialAssetGroup::new(
                MaterialAssetInfo::new("custom").expect("test suffix should be valid"),
                overrides,
            ),
            description,
        )))
    }

    #[test]
    fn registry_reference_round_trips_both_codecs() {
        init_test_registry();
        let component =
            ProvidesTrimMaterial::new(RegistryHolder::reference(&vanilla_trim_materials::IRON));

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("registry trim material should encode");
        assert_eq!(
            ProvidesTrimMaterial::read(&mut Cursor::new(network.as_slice()))
                .expect("registry trim material should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(nbt, simdnbt::owned::NbtTag::String("minecraft:iron".into()));
        assert_eq!(parse_component(nbt), Some(component));
    }

    #[test]
    fn inline_material_round_trips_and_hashes_its_flattened_record() {
        init_test_registry();
        let component = inline_component();

        let mut network = Vec::new();
        component
            .write(&mut network)
            .expect("inline trim material should encode");
        assert_eq!(
            ProvidesTrimMaterial::read(&mut Cursor::new(network.as_slice()))
                .expect("inline trim material should decode"),
            component
        );

        let nbt = component.clone().to_nbt_tag();
        assert_eq!(parse_component(nbt.clone()), Some(component.clone()));
        assert_eq!(component.compute_hash(), nbt.compute_hash());

        let simdnbt::owned::NbtTag::Compound(compound) = nbt else {
            panic!("inline trim material should encode as a compound");
        };
        assert_eq!(
            compound
                .compound("description")
                .and_then(|description| description.string("color"))
                .map(|color| color.to_str().into_owned()),
            Some("#123456".to_owned())
        );
    }

    #[test]
    fn invalid_asset_suffixes_are_rejected_by_both_codecs() {
        init_test_registry();
        assert!(MaterialAssetInfo::new("Bad Suffix").is_err());

        let mut network = Vec::new();
        VarInt(0)
            .write(&mut network)
            .expect("direct holder discriminator should encode");
        "Bad Suffix"
            .write_prefixed::<VarInt>(&mut network)
            .expect("invalid test suffix should encode as a string");
        VarInt(0)
            .write(&mut network)
            .expect("empty override map should encode");
        TextComponent::plain("Invalid material")
            .write(&mut network)
            .expect("description should encode");
        assert!(ProvidesTrimMaterial::read(&mut Cursor::new(network.as_slice())).is_err());

        let mut invalid = simdnbt::owned::NbtCompound::new();
        invalid.insert("asset_name", "Bad Suffix");
        invalid.insert("description", "Invalid material");
        assert!(parse_component(simdnbt::owned::NbtTag::Compound(invalid)).is_none());
    }

    #[test]
    fn extracted_item_prototypes_reference_every_vanilla_trim_material() {
        init_test_registry();
        let prototypes = [
            (
                &*vanilla_items::REDSTONE,
                &*vanilla_trim_materials::REDSTONE,
            ),
            (&*vanilla_items::DIAMOND, &*vanilla_trim_materials::DIAMOND),
            (&*vanilla_items::EMERALD, &*vanilla_trim_materials::EMERALD),
            (
                &*vanilla_items::LAPIS_LAZULI,
                &*vanilla_trim_materials::LAPIS,
            ),
            (&*vanilla_items::QUARTZ, &*vanilla_trim_materials::QUARTZ),
            (
                &*vanilla_items::AMETHYST_SHARD,
                &*vanilla_trim_materials::AMETHYST,
            ),
            (&*vanilla_items::IRON_INGOT, &*vanilla_trim_materials::IRON),
            (
                &*vanilla_items::COPPER_INGOT,
                &*vanilla_trim_materials::COPPER,
            ),
            (&*vanilla_items::GOLD_INGOT, &*vanilla_trim_materials::GOLD),
            (
                &*vanilla_items::NETHERITE_INGOT,
                &*vanilla_trim_materials::NETHERITE,
            ),
            (
                &*vanilla_items::RESIN_BRICK,
                &*vanilla_trim_materials::RESIN,
            ),
        ];

        assert_eq!(
            REGISTRY
                .items
                .iter()
                .filter(|(_, item)| item.components.has(PROVIDES_TRIM_MATERIAL))
                .count(),
            prototypes.len()
        );
        for (item, material) in prototypes {
            assert_eq!(
                ItemStack::new(item)
                    .get(PROVIDES_TRIM_MATERIAL)
                    .and_then(|component| component.material().as_reference()),
                Some(material)
            );
        }
    }
}
