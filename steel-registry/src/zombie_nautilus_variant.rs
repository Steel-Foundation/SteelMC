use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

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

impl ToNbtTag for ZombieNautilusVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList};
        let mut compound = NbtCompound::new();
        let asset_id = self.asset_id.to_string();
        compound.insert("asset_id", asset_id.as_str());
        compound.insert("baby_asset_id", asset_id.as_str());
        if let Some(model) = self.model {
            compound.insert("model", model);
        }
        let conditions: Vec<NbtCompound> = self
            .spawn_conditions
            .iter()
            .map(|entry| {
                let mut e = NbtCompound::new();
                e.insert("priority", entry.priority);
                if let Some(cond) = &entry.condition {
                    let mut c = NbtCompound::new();
                    c.insert("type", cond.condition_type);
                    c.insert("biomes", cond.biomes);
                    e.insert("condition", NbtTag::Compound(c));
                }
                e
            })
            .collect();
        compound.insert(
            "spawn_conditions",
            NbtTag::List(NbtList::Compound(conditions)),
        );
        NbtTag::Compound(compound)
    }
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

    pub fn iter(&self) -> impl Iterator<Item = (usize, ZombieNautilusVariantRef)> + '_ {
        self.zombie_nautilus_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for ZombieNautilusVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    ZombieNautilusVariantRegistry,
    ZombieNautilusVariant,
    zombie_nautilus_variants_by_id,
    zombie_nautilus_variants_by_key,
    zombie_nautilus_variants
);
