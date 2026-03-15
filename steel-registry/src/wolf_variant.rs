use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a full wolf variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct WolfVariant {
    pub key: Identifier,
    pub assets: WolfAssetInfo,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// Contains the texture resource locations for a wolf variant.
#[derive(Debug)]
pub struct WolfAssetInfo {
    pub wild: Identifier,
    pub tame: Identifier,
    pub angry: Identifier,
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

pub type WolfVariantRef = &'static WolfVariant;

pub struct WolfVariantRegistry {
    wolf_variants_by_id: Vec<WolfVariantRef>,
    wolf_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl WolfVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            wolf_variants_by_id: Vec::new(),
            wolf_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, wolf_variant: WolfVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register wolf variants after the registry has been frozen"
        );

        let id = self.wolf_variants_by_id.len();
        self.wolf_variants_by_key
            .insert(wolf_variant.key.clone(), id);
        self.wolf_variants_by_id.push(wolf_variant);
        id
    }

    /// Replaces a wolf_variant at a given index.
    /// Returns true if the wolf_variant was replaced and false if the wolf_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, wolf_variant: WolfVariantRef, id: usize) -> bool {
        if id >= self.wolf_variants_by_id.len() {
            return false;
        }
        self.wolf_variants_by_id[id] = wolf_variant;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, WolfVariantRef)> + '_ {
        self.wolf_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for WolfVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    WolfVariantRegistry,
    WolfVariant,
    wolf_variants_by_id,
    wolf_variants_by_key,
    wolf_variants
);
