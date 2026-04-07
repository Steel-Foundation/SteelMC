use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a full wolf variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct WolfVariant {
    pub key: Identifier,
    pub assets: WolfAssetInfo,
    pub baby_assets: WolfAssetInfo,
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

impl ToNbtTag for &WolfVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList};
        let mut compound = NbtCompound::new();
        let mut assets = NbtCompound::new();
        let wild = self.assets.wild.to_string();
        assets.insert("wild", wild.as_str());
        let tame = self.assets.tame.to_string();
        assets.insert("tame", tame.as_str());
        let angry = self.assets.angry.to_string();
        assets.insert("angry", angry.as_str());
        compound.insert("assets", NbtTag::Compound(assets));
        let mut baby_assets = NbtCompound::new();
        let wild = self.assets.wild.to_string();
        baby_assets.insert("wild", wild.as_str());
        let tame = self.assets.tame.to_string();
        baby_assets.insert("tame", tame.as_str());
        let angry = self.assets.angry.to_string();
        baby_assets.insert("angry", angry.as_str());
        compound.insert("baby_assets", NbtTag::Compound(baby_assets));
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
}

crate::impl_standard_methods!(
    WolfVariantRegistry,
    WolfVariantRef,
    wolf_variants_by_id,
    wolf_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    WolfVariantRegistry,
    WolfVariant,
    wolf_variants_by_id,
    wolf_variants_by_key,
    wolf_variants
);
