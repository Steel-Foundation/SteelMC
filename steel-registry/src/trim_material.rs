use crate::RegistryExt;
use rustc_hash::FxHashMap;
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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<TrimMaterialRef> {
        self.trim_materials_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, trim_material: TrimMaterialRef) -> &usize {
        self.trim_materials_by_key
            .get(&trim_material.key)
            .expect("Trim material not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<TrimMaterialRef> {
        self.trim_materials_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, TrimMaterialRef)> + '_ {
        self.trim_materials_by_id
            .iter()
            .enumerate()
            .map(|(id, &material)| (id, material))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.trim_materials_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.trim_materials_by_id.is_empty()
    }
}

impl RegistryExt for TrimMaterialRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for TrimMaterialRegistry {
    fn default() -> Self {
        Self::new()
    }
}
