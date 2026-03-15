use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full frog variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct FrogVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub spawn_conditions: &'static [SpawnConditionEntry],
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

pub type FrogVariantRef = &'static FrogVariant;

pub struct FrogVariantRegistry {
    frog_variants_by_id: Vec<FrogVariantRef>,
    frog_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl FrogVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            frog_variants_by_id: Vec::new(),
            frog_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, frog_variant: FrogVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register frog variants after the registry has been frozen"
        );

        let id = self.frog_variants_by_id.len();
        self.frog_variants_by_key
            .insert(frog_variant.key.clone(), id);
        self.frog_variants_by_id.push(frog_variant);
        id
    }

    /// Replaces a frog at a given index.
    /// Returns true if the frog was replaced and false if the frog wasn't replaced
    #[must_use]
    pub fn replace(&mut self, frog: FrogVariantRef, id: usize) -> bool {
        if id >= self.frog_variants_by_id.len() {
            return false;
        }
        self.frog_variants_by_id[id] = frog;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, FrogVariantRef)> + '_ {
        self.frog_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for FrogVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    FrogVariantRegistry,
    FrogVariant,
    frog_variants_by_id,
    frog_variants_by_key,
    frog_variants
);
