use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents an armor trim material definition from the data packs.
#[derive(Debug)]
pub struct TrimMaterial {
    pub key: Identifier,
    pub asset_name: String,
    pub description: StyledTextComponent,
    pub override_armor_assets: FxHashMap<Identifier, String>,
}

/// Represents a translatable text component that can also include styling.
#[derive(Debug)]
pub struct StyledTextComponent {
    pub translate: String,
    pub color: Option<String>,
}

impl ToNbtTag for &TrimMaterial {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::NbtCompound;
        let mut compound = NbtCompound::new();
        compound.insert("asset_name", self.asset_name.as_str());
        let mut desc = NbtCompound::new();
        desc.insert("translate", self.description.translate.as_str());
        if let Some(color) = &self.description.color {
            desc.insert("color", color.as_str());
        }
        compound.insert("description", NbtTag::Compound(desc));
        let mut overrides = NbtCompound::new();
        for (key, value) in &self.override_armor_assets {
            let key_str = key.to_string();
            overrides.insert(key_str.as_str(), value.as_str());
        }
        compound.insert("override_armor_assets", NbtTag::Compound(overrides));
        NbtTag::Compound(compound)
    }
}

pub type TrimMaterialRef = &'static TrimMaterial;

pub struct TrimMaterialRegistry {
    trim_materials_by_id: Vec<TrimMaterialRef>,
    trim_materials_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl TrimMaterialRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            trim_materials_by_id: Vec::new(),
            trim_materials_by_key: FxHashMap::default(),
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
