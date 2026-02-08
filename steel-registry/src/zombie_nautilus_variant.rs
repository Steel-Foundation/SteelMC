use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

/// Represents a full zombie nautilus variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct ZombieNautilusVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub model: Option<&'static str>,
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

pub type ZombieNautilusVariantRef = &'static ZombieNautilusVariant;

pub struct ZombieNautilusVariantRegistry {
    zombie_nautilus_variants_by_id: Vec<ZombieNautilusVariantRef>,
    zombie_nautilus_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ZombieNautilusVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            zombie_nautilus_variants_by_id: Vec::new(),
            zombie_nautilus_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, zombie_nautilus_variant: ZombieNautilusVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register zombie nautilus variants after the registry has been frozen"
        );

        let id = self.zombie_nautilus_variants_by_id.len();
        self.zombie_nautilus_variants_by_key
            .insert(zombie_nautilus_variant.key.clone(), id);
        self.zombie_nautilus_variants_by_id
            .push(zombie_nautilus_variant);
        id
    }

    /// Replaces a zombie_nautilus_variant at a given index.
    /// Returns true if the zombie_nautilus_variant was replaced and false if the zombie_nautilus_variant wasn't replaced
    #[must_use]
    pub fn replace(
        &mut self,
        zombie_nautilus_variant: ZombieNautilusVariantRef,
        id: usize,
    ) -> bool {
        if id >= self.zombie_nautilus_variants_by_id.len() {
            return false;
        }
        self.zombie_nautilus_variants_by_id[id] = zombie_nautilus_variant;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ZombieNautilusVariantRef> {
        self.zombie_nautilus_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, zombie_nautilus_variant: ZombieNautilusVariantRef) -> &usize {
        self.zombie_nautilus_variants_by_key
            .get(&zombie_nautilus_variant.key)
            .expect("Zombie nautilus variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ZombieNautilusVariantRef> {
        self.zombie_nautilus_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ZombieNautilusVariantRef)> + '_ {
        self.zombie_nautilus_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.zombie_nautilus_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.zombie_nautilus_variants_by_id.is_empty()
    }
}

impl RegistryExt for ZombieNautilusVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for ZombieNautilusVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
