use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full cat variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct CatVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
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

impl ToNbtTag for &CatVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        let conditions: Vec<NbtCompound> = self
            .spawn_conditions
            .iter()
            .map(|entry| {
                let mut e = NbtCompound::new();
                e.insert("priority", entry.priority);
                if let Some(cond) = &entry.condition {
                    let mut c = NbtCompound::new();
                    match cond {
                        SpawnCondition::Structure { structures } => {
                            c.insert("type", "minecraft:in_structure");
                            c.insert("structures", *structures);
                        }
                        SpawnCondition::MoonBrightness { min, max } => {
                            c.insert("type", "minecraft:moon_brightness");
                            if let Some(min) = min {
                                c.insert("min", *min);
                            }
                            if let Some(max) = max {
                                c.insert("max", *max);
                            }
                        }
                        SpawnCondition::Biome { biomes } => {
                            c.insert("type", "minecraft:biome");
                            c.insert("biomes", *biomes);
                        }
                    }
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
