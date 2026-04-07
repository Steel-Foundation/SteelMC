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

impl ToNbtTag for &ZombieNautilusVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
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
}

crate::impl_standard_methods!(
    ZombieNautilusVariantRegistry,
    ZombieNautilusVariantRef,
    zombie_nautilus_variants_by_id,
    zombie_nautilus_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    ZombieNautilusVariantRegistry,
    ZombieNautilusVariant,
    zombie_nautilus_variants_by_id,
    zombie_nautilus_variants_by_key,
    zombie_nautilus_variants
);
