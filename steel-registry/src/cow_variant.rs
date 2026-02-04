use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full cow variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct CowVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub model: CowModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the cow, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CowModelType {
    #[default]
    Normal,
    Cold,
    Warm,
}

/// A single entry in the list of spawn conditions.
#[derive(Debug)]
pub struct SpawnConditionEntry {
    pub priority: i32,
    pub condition: Option<BiomeCondition>,
}

/// Defines a condition based on a biome or list of biomes.
#[derive(Debug)]
pub struct BiomeCondition {
    pub condition_type: &'static str,
    pub biomes: &'static str,
}

pub type CowVariantRef = &'static CowVariant;

pub struct CowVariantRegistry {
    cow_variants_by_id: Vec<CowVariantRef>,
    cow_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CowVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cow_variants_by_id: Vec::new(),
            cow_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, cow_variant: CowVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register cow variants after the registry has been frozen"
        );

        let id = self.cow_variants_by_id.len();
        self.cow_variants_by_key.insert(cow_variant.key.clone(), id);
        self.cow_variants_by_id.push(cow_variant);
        id
    }

    /// Replaces a cow at a given index.
    /// Returns true if the cow was replaced and false if the cow wasn't replaced
    #[must_use]
    pub fn replace(&mut self, cow: CowVariantRef, id: usize) -> bool {
        if id >= self.cow_variants_by_id.len() {
            return false;
        }
        self.cow_variants_by_id[id] = cow;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<CowVariantRef> {
        self.cow_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, cow_variant: CowVariantRef) -> &usize {
        self.cow_variants_by_key
            .get(&cow_variant.key)
            .expect("Cow variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<CowVariantRef> {
        self.cow_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, CowVariantRef)> + '_ {
        self.cow_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cow_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cow_variants_by_id.is_empty()
    }
}

impl RegistryExt for CowVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for CowVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
