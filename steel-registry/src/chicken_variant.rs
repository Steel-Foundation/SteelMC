use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full chicken variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct ChickenVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub model: ChickenModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the chicken, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChickenModelType {
    #[default]
    Normal,
    Cold,
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

pub type ChickenVariantRef = &'static ChickenVariant;

pub struct ChickenVariantRegistry {
    chicken_variants_by_id: Vec<ChickenVariantRef>,
    chicken_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ChickenVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            chicken_variants_by_id: Vec::new(),
            chicken_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, chicken_variant: ChickenVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register chicken variants after the registry has been frozen"
        );

        let id = self.chicken_variants_by_id.len();
        self.chicken_variants_by_key
            .insert(chicken_variant.key.clone(), id);
        self.chicken_variants_by_id.push(chicken_variant);
        id
    }

    /// Replaces a chicken at a given index.
    /// Returns true if the chicken was replaced and false if the chicken wasn't replaced
    #[must_use]
    pub fn replace(&mut self, chicken: ChickenVariantRef, id: usize) -> bool {
        if id >= self.chicken_variants_by_id.len() {
            return false;
        }
        self.chicken_variants_by_id[id] = chicken;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ChickenVariantRef)> + '_ {
        self.chicken_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for ChickenVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    ChickenVariantRegistry,
    ChickenVariant,
    chicken_variants_by_id,
    chicken_variants_by_key,
    chicken_variants
);
