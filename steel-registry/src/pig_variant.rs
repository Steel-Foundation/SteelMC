use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full pig variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct PigVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
    pub model: PigModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the pig, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PigModelType {
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

impl ToNbtTag for &PigVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        compound.insert(
            "model",
            match self.model {
                PigModelType::Normal => "normal",
                PigModelType::Cold => "cold",
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

pub type PigVariantRef = &'static PigVariant;

pub struct PigVariantRegistry {
    pig_variants_by_id: Vec<PigVariantRef>,
    pig_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl PigVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pig_variants_by_id: Vec::new(),
            pig_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, pig_variant: PigVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register pig variants after the registry has been frozen"
        );

        let id = self.pig_variants_by_id.len();
        self.pig_variants_by_key.insert(pig_variant.key.clone(), id);
        self.pig_variants_by_id.push(pig_variant);
        id
    }

    /// Replaces a pig_variant at a given index.
    /// Returns true if the pig_variant was replaced and false if the pig_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, pig_variant: PigVariantRef, id: usize) -> bool {
        if id >= self.pig_variants_by_id.len() {
            return false;
        }
        self.pig_variants_by_id[id] = pig_variant;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PigVariantRef)> + '_ {
        self.pig_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for PigVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    PigVariantRegistry,
    PigVariant,
    pig_variants_by_id,
    pig_variants_by_key,
    pig_variants
);
