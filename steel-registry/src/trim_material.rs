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

    pub fn register(&mut self, trim_material: TrimMaterialRef, key: Identifier) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register trim materials after the registry has been frozen"
        );

        let id = self.trim_materials_by_id.len();
        self.trim_materials_by_key.insert(key, id);
        self.trim_materials_by_id.push(trim_material);
        id
    }

    /// Replaces a trim_material at a given index.
    /// Returns true if the trim_material was replaced and false if the trim_material wasn't replaced
    #[must_use]
    pub fn replace(&mut self, trim_material: TrimMaterialRef, id: usize) -> bool {
        if id >= self.trim_materials_by_id.len() {
            return false;
        }
        self.trim_materials_by_id[id] = trim_material;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, TrimMaterialRef)> + '_ {
        self.trim_materials_by_id
            .iter()
            .enumerate()
            .map(|(id, &material)| (id, material))
    }
}

impl Default for TrimMaterialRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    TrimMaterialRegistry,
    TrimMaterial,
    trim_materials_by_id,
    trim_materials_by_key,
    trim_materials
);
