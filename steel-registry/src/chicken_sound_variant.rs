use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

/// Represents a set of sounds for a chicken variant from a data pack JSON file.
#[derive(Debug)]
pub struct ChickenSoundVariant {
    pub key: Identifier,
    pub baby_sounds: ChickenAge,
    pub adult_sounds: ChickenAge,
}
#[derive(Debug)]
pub struct ChickenAge {
    pub ambient_sound: Identifier,
    pub death_sound: Identifier,
    pub hurt_sound: Identifier,
    pub step_sound: Identifier,
}

impl ChickenAge {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut adult = NbtCompound::new();
        let s = self.ambient_sound.to_string();
        adult.insert("ambient_sound", s.as_str());
        let s = self.death_sound.to_string();
        adult.insert("death_sound", s.as_str());
        let s = self.hurt_sound.to_string();
        adult.insert("hurt_sound", s.as_str());
        let s = self.step_sound.to_string();
        adult.insert("step_sound", s.as_str());
        NbtTag::Compound(adult)
    }
}

impl ChickenSoundVariant {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt());
        NbtTag::Compound(compound)
    }
}

pub type ChickenSoundVariantRef = &'static ChickenSoundVariant;

pub struct ChickenSoundVariantRegistry {
    chicken_sound_variants_by_id: Vec<ChickenSoundVariantRef>,
    chicken_sound_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl ChickenSoundVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            chicken_sound_variants_by_id: Vec::new(),
            chicken_sound_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, chicken_sound_variant: ChickenSoundVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register chicken sound variants after the registry has been frozen"
        );

        let id = self.chicken_sound_variants_by_id.len();
        self.chicken_sound_variants_by_key
            .insert(chicken_sound_variant.key.clone(), id);
        self.chicken_sound_variants_by_id
            .push(chicken_sound_variant);
        id
    }

    /// Replaces a chicken_sound_variant at a given index.
    /// Returns true if the chicken_sound_variant was replaced and false if the chicken_sound_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, chicken_sound_variant: ChickenSoundVariantRef, id: usize) -> bool {
        if id >= self.chicken_sound_variants_by_id.len() {
            return false;
        }
        self.chicken_sound_variants_by_id[id] = chicken_sound_variant;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<ChickenSoundVariantRef> {
        self.chicken_sound_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, chicken_sound_variant: ChickenSoundVariantRef) -> &usize {
        self.chicken_sound_variants_by_key
            .get(&chicken_sound_variant.key)
            .expect("chicken sound variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<ChickenSoundVariantRef> {
        self.chicken_sound_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, ChickenSoundVariantRef)> + '_ {
        self.chicken_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.chicken_sound_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chicken_sound_variants_by_id.is_empty()
    }
}

impl RegistryExt for ChickenSoundVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for ChickenSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
