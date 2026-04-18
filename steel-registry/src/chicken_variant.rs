use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full chicken variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct ChickenVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
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

impl ToNbtTag for &ChickenVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        compound.insert(
            "model",
            match self.model {
                ChickenModelType::Normal => "normal",
                ChickenModelType::Cold => "cold",
            },
        );
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
}

crate::impl_standard_methods!(
    ChickenVariantRegistry,
    ChickenVariantRef,
    chicken_variants_by_id,
    chicken_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    ChickenVariantRegistry,
    ChickenVariant,
    chicken_variants_by_id,
    chicken_variants_by_key,
    chicken_variants
);
