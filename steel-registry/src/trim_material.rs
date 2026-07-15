//! Armor trim material registry values.

use std::fmt::{self, Display, Formatter};
use std::io::{Cursor, Error, Result as IoResult, Write};

use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use simdnbt::{FromNbtTag, ToNbtTag};
use steel_utils::Identifier;
use steel_utils::codec::VarInt;
use steel_utils::hash::{ComponentHasher, HashComponent, HashEntry, sort_map_entries};
use steel_utils::serial::{PrefixedRead, PrefixedWrite, ReadFrom, WriteTo};
use text_components::TextComponent;

use crate::{REGISTRY, RegistryExt, RegistryHolderEntry};

/// Texture suffix used by an armor trim material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialAssetInfo {
    suffix: String,
}

impl MaterialAssetInfo {
    /// Creates an asset suffix accepted by Vanilla's resource-path codec.
    pub fn new(suffix: impl Into<String>) -> Result<Self, InvalidMaterialAssetInfo> {
        let suffix = suffix.into();
        if !Identifier::validate_path(&suffix) {
            return Err(InvalidMaterialAssetInfo { suffix });
        }
        Ok(Self { suffix })
    }

    pub(crate) fn from_validated_suffix(suffix: String) -> Self {
        assert!(
            Identifier::validate_path(&suffix),
            "generated trim material contains an invalid asset suffix"
        );
        Self { suffix }
    }

    #[must_use]
    pub fn suffix(&self) -> &str {
        &self.suffix
    }
}

impl WriteTo for MaterialAssetInfo {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.suffix.write_prefixed::<VarInt>(writer)
    }
}

impl ReadFrom for MaterialAssetInfo {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Self::new(String::read_prefixed::<VarInt>(data)?).map_err(Error::other)
    }
}

impl HashComponent for MaterialAssetInfo {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        self.suffix.hash_component(hasher);
    }
}

/// Invalid value rejected by `ExtraCodecs.RESOURCE_PATH_CODEC`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidMaterialAssetInfo {
    suffix: String,
}

impl InvalidMaterialAssetInfo {
    #[must_use]
    pub fn suffix(&self) -> &str {
        &self.suffix
    }
}

impl Display for InvalidMaterialAssetInfo {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid string to use as a resource path element: {}",
            self.suffix
        )
    }
}

impl std::error::Error for InvalidMaterialAssetInfo {}

/// Base texture suffix and equipment-asset-specific overrides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialAssetGroup {
    base: MaterialAssetInfo,
    overrides: FxHashMap<Identifier, MaterialAssetInfo>,
}

impl MaterialAssetGroup {
    #[must_use]
    pub const fn new(
        base: MaterialAssetInfo,
        overrides: FxHashMap<Identifier, MaterialAssetInfo>,
    ) -> Self {
        Self { base, overrides }
    }

    #[must_use]
    pub const fn base(&self) -> &MaterialAssetInfo {
        &self.base
    }

    #[must_use]
    pub const fn overrides(&self) -> &FxHashMap<Identifier, MaterialAssetInfo> {
        &self.overrides
    }

    /// Resolves the suffix for an equipment asset, falling back to the base.
    #[must_use]
    pub fn asset_id(&self, equipment_asset: &Identifier) -> &MaterialAssetInfo {
        self.overrides.get(equipment_asset).unwrap_or(&self.base)
    }

    fn insert_nbt_fields(&self, compound: &mut NbtCompound) {
        compound.insert("asset_name", self.base.suffix());
        if self.overrides.is_empty() {
            return;
        }

        let mut overrides = NbtCompound::new();
        for (equipment_asset, asset) in &self.overrides {
            overrides.insert(equipment_asset.to_string(), asset.suffix());
        }
        compound.insert("override_armor_assets", NbtTag::Compound(overrides));
    }

    fn push_hash_fields(&self, entries: &mut Vec<HashEntry>) {
        push_hash_entry(entries, "asset_name", &self.base);
        if self.overrides.is_empty() {
            return;
        }

        let mut value_hasher = ComponentHasher::new();
        hash_overrides(&self.overrides, &mut value_hasher);
        push_prehashed_entry(entries, "override_armor_assets", value_hasher);
    }
}

impl WriteTo for MaterialAssetGroup {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.base.write(writer)?;
        let count = i32::try_from(self.overrides.len())
            .map_err(|_| Error::other("trim material override count exceeds protocol range"))?;
        VarInt(count).write(writer)?;
        for (equipment_asset, asset) in &self.overrides {
            equipment_asset.write(writer)?;
            asset.write(writer)?;
        }
        Ok(())
    }
}

impl ReadFrom for MaterialAssetGroup {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        let base = MaterialAssetInfo::read(data)?;
        let encoded_count = VarInt::read(data)?.0;
        let count = usize::try_from(encoded_count).map_err(|_| {
            Error::other(format!(
                "negative trim material override count: {encoded_count}"
            ))
        })?;
        let mut overrides = FxHashMap::default();
        for _ in 0..count {
            overrides.insert(Identifier::read(data)?, MaterialAssetInfo::read(data)?);
        }
        Ok(Self::new(base, overrides))
    }
}

/// Complete registry-independent trim material definition.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimMaterialValue {
    assets: MaterialAssetGroup,
    description: TextComponent,
}

impl TrimMaterialValue {
    #[must_use]
    pub const fn new(assets: MaterialAssetGroup, description: TextComponent) -> Self {
        Self {
            assets,
            description,
        }
    }

    #[must_use]
    pub const fn assets(&self) -> &MaterialAssetGroup {
        &self.assets
    }

    #[must_use]
    pub const fn description(&self) -> &TextComponent {
        &self.description
    }

    fn to_nbt_tag_ref(&self) -> NbtTag {
        let mut compound = NbtCompound::new();
        self.assets.insert_nbt_fields(&mut compound);
        compound.insert("description", self.description.to_codec_nbt());
        NbtTag::Compound(compound)
    }
}

impl WriteTo for TrimMaterialValue {
    fn write(&self, writer: &mut impl Write) -> IoResult<()> {
        self.assets.write(writer)?;
        WriteTo::write(&self.description.to_codec_nbt(), writer)
    }
}

impl ReadFrom for TrimMaterialValue {
    fn read(data: &mut Cursor<&[u8]>) -> IoResult<Self> {
        Ok(Self::new(
            MaterialAssetGroup::read(data)?,
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
        let base = MaterialAssetInfo::new(compound.string("asset_name")?.to_str()).ok()?;
        let mut overrides = FxHashMap::default();
        if let Some(tag) = compound.get("override_armor_assets") {
            for (equipment_asset, asset) in tag.compound()?.iter() {
                overrides.insert(
                    equipment_asset.to_str().parse().ok()?,
                    MaterialAssetInfo::new(asset.string()?.to_str()).ok()?,
                );
            }
        }
        let description = TextComponent::from_nbt(&compound.get("description")?.to_owned())?;
        Some(Self::new(
            MaterialAssetGroup::new(base, overrides),
            description,
        ))
    }
}

impl HashComponent for TrimMaterialValue {
    fn hash_component(&self, hasher: &mut ComponentHasher) {
        let mut entries = Vec::new();
        self.assets.push_hash_fields(&mut entries);
        push_hash_entry(&mut entries, "description", &self.description);
        hash_entries(entries, hasher);
    }
}

fn hash_overrides(
    overrides: &FxHashMap<Identifier, MaterialAssetInfo>,
    hasher: &mut ComponentHasher,
) {
    let mut entries = overrides
        .iter()
        .map(|(equipment_asset, asset)| {
            let mut key_hasher = ComponentHasher::new();
            equipment_asset.hash_component(&mut key_hasher);
            let mut value_hasher = ComponentHasher::new();
            asset.hash_component(&mut value_hasher);
            HashEntry::new(key_hasher, value_hasher)
        })
        .collect::<Vec<_>>();
    sort_map_entries(&mut entries);
    hash_entries(entries, hasher);
}

fn push_hash_entry<T: HashComponent + ?Sized>(entries: &mut Vec<HashEntry>, key: &str, value: &T) {
    let mut value_hasher = ComponentHasher::new();
    value.hash_component(&mut value_hasher);
    push_prehashed_entry(entries, key, value_hasher);
}

fn push_prehashed_entry(entries: &mut Vec<HashEntry>, key: &str, value_hasher: ComponentHasher) {
    let mut key_hasher = ComponentHasher::new();
    key.hash_component(&mut key_hasher);
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
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl TrimMaterialRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            trim_materials_by_id: Vec::new(),
            trim_materials_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
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
    use simdnbt::ToNbtTag as _;
    use steel_utils::Identifier;
    use text_components::format::Color;

    use crate::test_support::init_test_registry;
    use crate::{REGISTRY, vanilla_trim_materials};

    #[test]
    fn generated_materials_follow_vanilla_registry_order_and_asset_groups() {
        init_test_registry();
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
        assert_eq!(iron.assets().base().suffix(), "iron");
        assert_eq!(
            iron.assets()
                .asset_id(&Identifier::vanilla_static("iron"))
                .suffix(),
            "iron_darker"
        );
        assert_eq!(
            iron.assets()
                .asset_id(&Identifier::vanilla_static("diamond"))
                .suffix(),
            "iron"
        );
        assert_eq!(
            iron.description().format.color,
            Some(Color::Rgb(0xec, 0xec, 0xec))
        );
    }

    #[test]
    fn generated_definition_uses_the_current_flattened_persistent_shape() {
        init_test_registry();

        let simdnbt::owned::NbtTag::Compound(iron) = (&*vanilla_trim_materials::IRON).to_nbt_tag()
        else {
            panic!("trim material definition should encode as a compound");
        };
        assert_eq!(
            iron.string("asset_name")
                .map(|value| value.to_str().into_owned()),
            Some("iron".to_owned())
        );
        assert_eq!(
            iron.compound("override_armor_assets")
                .and_then(|overrides| overrides.string("minecraft:iron"))
                .map(|value| value.to_str().into_owned()),
            Some("iron_darker".to_owned())
        );
        assert_eq!(
            iron.compound("description")
                .and_then(|description| description.string("color"))
                .map(|value| value.to_str().into_owned()),
            Some("#ECECEC".to_owned())
        );

        let simdnbt::owned::NbtTag::Compound(quartz) =
            (&*vanilla_trim_materials::QUARTZ).to_nbt_tag()
        else {
            panic!("trim material definition should encode as a compound");
        };
        assert!(!quartz.contains("override_armor_assets"));
    }
}
