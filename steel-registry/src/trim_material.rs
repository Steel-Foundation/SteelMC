//! Armor trim material registry values.

use std::io::{Cursor, Result as IoResult, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::{REGISTRY, RegistryExt, RegistryHolderEntry, RegistryTags};

/// Complete registry-independent trim material definition.
///
/// Mirrors vanilla's `TrimMaterial(Identifier paletteId, Component description)`
/// record — the old per-equipment `asset_name`/`override_armor_assets` texture
/// suffix system was removed in favor of a single shared palette texture.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimMaterialValue {
    palette_id: Identifier,
    description: TextComponent,
}

impl TrimMaterialValue {
    #[must_use]
    pub const fn new(palette_id: Identifier, description: TextComponent) -> Self {
        Self {
            palette_id,
            description,
        }
    }

    #[must_use]
    pub const fn palette_id(&self) -> &Identifier {
        &self.palette_id
    }

    #[must_use]
    pub const fn description(&self) -> &TextComponent {
        &self.description
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        compound.insert("palette_id", self.palette_id.clone().to_nbt_tag());
        compound.insert("description", self.description.to_codec_nbt());
        NbtTag::Compound(compound)
    }
}

impl WriteTo for TrimMaterialValue {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.palette_id.write(writer)?;
        WriteTo::write(&self.description.to_codec_nbt(), writer)
    }
}

impl ReadFrom for TrimMaterialValue {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Ok(Self::new(
            Identifier::read(data)?,
            TextComponent::read(data)?,
        ))
    }
}

impl ToNbtTag for TrimMaterialValue {
    fn to_nbt_tag(self) -> NbtTag {
        self.to_nbt_tag_ref()
    }
}

impl FromNbtTag for TrimMaterialValue {
    fn from_nbt_tag(tag: simdnbt::borrow::NbtTag) -> Option<Self> {
        let compound = tag.compound()?;
        let palette_id = Identifier::from_nbt_tag(compound.get("palette_id")?)?;
        let description = TextComponent::from_nbt(&compound.get("description")?.to_owned())?;
        Some(Self::new(palette_id, description))
    }
}

impl HashComponent for TrimMaterialValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::with_capacity(2);
        push_hash_entry(&mut entries, "palette_id", &self.palette_id);
        push_hash_entry(&mut entries, "description", &self.description);
        hash_entries(entries, hasher);
    }
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut key_hasher = ComponentHasher::new();
    key_hasher.put_string(key);
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    entries.push(HashEntry::new(key_hasher, value_hasher));
}

fn hash_entries(mut entries: Vec<HashEntry>, hasher: &mut ComponentHasher) {
    sort_map_entries(&mut entries);
    hasher.start_map();
    for entry in &entries {
        hasher.put_raw_bytes(&entry.key_bytes);
        hasher.put_raw_bytes(&entry.value_bytes);
    }
    hasher.end_map();
}

/// Registered armor trim material definition.
#[derive(Debug)]
pub struct TrimMaterial {
    pub key: Identifier,
    value: TrimMaterialValue,
}

impl TrimMaterial {
    #[must_use]
    pub const fn new(key: Identifier, value: TrimMaterialValue) -> Self {
        Self { key, value }
    }

    #[must_use]
    pub const fn value(&self) -> &TrimMaterialValue {
        &self.value
    }
}

impl ToNbtTag for &TrimMaterial {
    fn to_nbt_tag(self) -> NbtTag {
        self.value.to_nbt_tag_ref()
    }
}

pub type TrimMaterialRef = &'static TrimMaterial;

pub struct TrimMaterialRegistry {
    trim_materials_by_id: Vec<TrimMaterialRef>,
    trim_materials_by_key: FxHashMap<Identifier, usize>,
    tags: RegistryTags,
    allows_registering: bool,
}

impl TrimMaterialRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            trim_materials_by_id: Vec::new(),
            trim_materials_by_key: FxHashMap::default(),
            tags: RegistryTags::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    TrimMaterialRegistry,
    TrimMaterialRef,
    trim_materials_by_id,
    trim_materials_by_key,
    allows_registering
);

crate::impl_registry!(
    TrimMaterialRegistry,
    TrimMaterial,
    trim_materials_by_id,
    trim_materials_by_key,
    trim_materials
);
crate::impl_tagged_registry!(TrimMaterialRegistry, trim_materials_by_key, "trim material");

impl RegistryHolderEntry for TrimMaterial {
    type Value = TrimMaterialValue;

    const REGISTRY_NAME: &'static str = "trim material";

    fn holder_value(&self) -> &Self::Value {
        &self.value
    }

    fn holder_by_id(id: usize) -> Option<&'static Self> {
        REGISTRY.trim_materials.by_id(id)
    }

    fn holder_by_key(key: &Identifier) -> Option<&'static Self> {
        REGISTRY.trim_materials.by_key(key)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;
    use text_components::format::Color;

    use crate::init_vanilla_registry;
    use crate::{REGISTRY, vanilla_trim_materials};

    #[test]
    fn generated_materials_follow_vanilla_registry_order_and_palette_ids() {
        init_vanilla_registry();
        let keys = REGISTRY
            .trim_materials
            .iter()
            .map(|(_, material)| material.key.path.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "quartz",
                "iron",
                "netherite",
                "redstone",
                "copper",
                "gold",
                "emerald",
                "diamond",
                "lapis",
                "amethyst",
                "resin",
            ]
        );

        let iron = vanilla_trim_materials::IRON.value();
        assert_eq!(iron.palette_id(), &Identifier::vanilla_static("trim/iron"));
        assert_eq!(
            iron.description().format.color,
            Some(Color::Rgb(0xec, 0xec, 0xec))
        );
    }

    #[test]
    fn generated_definition_uses_the_flattened_persistent_shape() {
        use simdnbt::ToNbtTag as _;

        init_vanilla_registry();

        let simdnbt::owned::NbtTag::Compound(iron) = (&*vanilla_trim_materials::IRON).to_nbt_tag()
        else {
            panic!("trim material definition should encode as a compound");
        };
        assert_eq!(
            iron.string("palette_id")
                .map(|value| value.to_str().into_owned()),
            Some("minecraft:trim/iron".to_owned())
        );
        assert_eq!(
            iron.compound("description")
                .and_then(|description| description.string("color"))
                .map(|value| value.to_str().into_owned()),
            Some("#ECECEC".to_owned())
        );
    }
}
