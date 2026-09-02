use steel_utils::random::Random;

use crate::biome::BiomeRef;
use crate::shared_structs::pick_spawn_conditioned_entry;
use crate::shared_structs::{SpawnConditionEntry, insert_spawn_conditions};
use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full cow variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct CowVariant {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub baby_asset_id: Identifier,
    pub model: CowModelType,
    pub spawn_conditions: &'static [SpawnConditionEntry],
}

/// The model type for the cow, which can affect its shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CowModelType {
    #[default]
    Normal,
    Cold,
    Warm,
}

impl ToNbtTag for &CowVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("asset_id", self.asset_id.clone());
        compound.insert("baby_asset_id", self.baby_asset_id.clone());
        compound.insert(
            "model",
            match self.model {
                CowModelType::Normal => "normal",
                CowModelType::Cold => "cold",
                CowModelType::Warm => "warm",
            },
        );
        insert_spawn_conditions(&mut compound, self.spawn_conditions);
        NbtTag::Compound(compound)
    }
}

pub type CowVariantRef = &'static CowVariant;

pub struct CowVariantRegistry {
    cow_variants_by_id: Vec<CowVariantRef>,
    cow_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CowVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cow_variants_by_id: Vec::new(),
            cow_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    #[must_use]
    pub fn select_spawn_variant(
        &self,
        biome: BiomeRef,
        random: &mut impl Random,
    ) -> Option<CowVariantRef> {
        // Mirrors vanilla conditioned variant selection against the spawn biome.
        pick_spawn_conditioned_entry(
            self.iter().map(|(_, variant)| variant),
            |variant| variant.spawn_conditions,
            biome,
            random,
        )
    }
}

crate::impl_standard_methods!(
    CowVariantRegistry,
    CowVariantRef,
    cow_variants_by_id,
    cow_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    CowVariantRegistry,
    CowVariant,
    cow_variants_by_id,
    cow_variants_by_key,
    cow_variants
);
