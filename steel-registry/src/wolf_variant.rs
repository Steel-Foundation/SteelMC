use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

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

impl WolfVariant {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<WolfVariantRef> {
        self.wolf_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, wolf_variant: WolfVariantRef) -> &usize {
        self.wolf_variants_by_key
            .get(&wolf_variant.key)
            .expect("Wolf variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<WolfVariantRef> {
        self.wolf_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, WolfVariantRef)> + '_ {
        self.wolf_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.wolf_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wolf_variants_by_id.is_empty()
    }
}

impl RegistryExt for WolfVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for WolfVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
