use rustc_hash::FxHashMap;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::Identifier;

use crate::RegistryExt;

/// Represents a set of sounds for a pig variant from a data pack JSON file.
#[derive(Debug)]
pub struct PigSoundVariant {
    pub key: Identifier,
    pub adult_sounds: PigAge,
    pub baby_sounds: PigAge,
}
#[derive(Debug)]
pub struct PigAge {
    pub ambient_sound: Identifier,
    pub death_sound: Identifier,
    pub hurt_sound: Identifier,
    pub eat_sound: Identifier,
    pub step_sound: Identifier,
}
impl PigAge {
    pub fn to_nbt(&self) -> NbtTag {
        let mut component = NbtCompound::new();
        let s = self.ambient_sound.to_string();
        component.insert("ambient_sound", s.as_str());
        let s = self.death_sound.to_string();
        component.insert("death_sound", s.as_str());
        let s = self.hurt_sound.to_string();
        component.insert("hurt_sound", s.as_str());
        let s = self.step_sound.to_string();
        component.insert("step_sound", s.as_str());
        let s = self.eat_sound.to_string();
        component.insert("eat_sound", s.as_str());
        NbtTag::Compound(component)
    }
}

impl PigSoundVariant {
    pub fn to_nbt(&self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt());
        NbtTag::Compound(compound)
    }
}

pub type PigSoundVariantRef = &'static PigSoundVariant;

pub struct PigSoundVariantRegistry {
    pig_sound_variants_by_id: Vec<PigSoundVariantRef>,
    pig_sound_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl PigSoundVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pig_sound_variants_by_id: Vec::new(),
            pig_sound_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, pig_sound_variant: PigSoundVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register pig sound variants after the registry has been frozen"
        );

        let id = self.pig_sound_variants_by_id.len();
        self.pig_sound_variants_by_key
            .insert(pig_sound_variant.key.clone(), id);
        self.pig_sound_variants_by_id.push(pig_sound_variant);
        id
    }

    /// Replaces a pig_sound_variant at a given index.
    /// Returns true if the pig_sound_variant was replaced and false if the pig_sound_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, pig_sound_variant: PigSoundVariantRef, id: usize) -> bool {
        if id >= self.pig_sound_variants_by_id.len() {
            return false;
        }
        self.pig_sound_variants_by_id[id] = pig_sound_variant;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<PigSoundVariantRef> {
        self.pig_sound_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, pig_sound_variant: PigSoundVariantRef) -> &usize {
        self.pig_sound_variants_by_key
            .get(&pig_sound_variant.key)
            .expect("pig sound variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<PigSoundVariantRef> {
        self.pig_sound_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PigSoundVariantRef)> + '_ {
        self.pig_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.pig_sound_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pig_sound_variants_by_id.is_empty()
    }
}

impl RegistryExt for PigSoundVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for PigSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
