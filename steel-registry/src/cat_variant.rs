use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full cat variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct CatVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// A single entry in the list of spawn conditions.
#[derive(Debug)]
pub struct SpawnConditionEntry {
    pub priority: i32,
    pub condition: Option<SpawnCondition>,
}

/// Defines various spawn conditions for cat variants.
#[derive(Debug)]
pub enum SpawnCondition {
    Structure { structures: &'static str },
    MoonBrightness { min: Option<f32>, max: Option<f32> },
    Biome { biomes: &'static str },
}

pub type CatVariantRef = &'static CatVariant;

pub struct CatVariantRegistry {
    cat_variants_by_id: Vec<CatVariantRef>,
    cat_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CatVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cat_variants_by_id: Vec::new(),
            cat_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, cat_variant: CatVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register cat variants after the registry has been frozen"
        );

        let id = self.cat_variants_by_id.len();
        self.cat_variants_by_key.insert(cat_variant.key.clone(), id);
        self.cat_variants_by_id.push(cat_variant);
        id
    }

    /// Replaces a cat_variant at a given index.
    /// Returns true if the cat_variant was replaced and false if the cat_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, cat_variant: CatVariantRef, id: usize) -> bool {
        if id >= self.cat_variants_by_id.len() {
            return false;
        }
        self.cat_variants_by_id[id] = cat_variant;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, CatVariantRef)> + '_ {
        self.cat_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for CatVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    CatVariantRegistry,
    CatVariant,
    cat_variants_by_id,
    cat_variants_by_key,
    cat_variants
);
