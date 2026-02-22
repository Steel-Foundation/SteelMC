use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

/// Represents a set of sounds for a cat variant from a data pack JSON file.
#[derive(Debug)]
pub struct CatSoundVariant {
    pub key: Identifier,
    pub adult_sounds: CatAge,
    pub baby_sounds: CatAge,
}
#[derive( Debug)]
pub struct CatAge {
    pub ambient_sound: Identifier,
    pub beg_for_food_sound: Identifier,
    pub death_sound: Identifier,
    pub eat_sound: Identifier,
    pub hiss_sound: Identifier,
    pub hurt_sound: Identifier,
    pub purr_sound: Identifier,
    pub purreow_sound: Identifier,
    pub stray_ambient_sound: Identifier,
}

impl CatAge{
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut component = NbtCompound::new();
        let s = self.ambient_sound.to_string();
        component.insert("ambient_sound", s.as_str());
        let s = self.beg_for_food_sound.to_string();
        component.insert("beg_for_food_sound", s.as_str());
        let s = self.death_sound.to_string();
        component.insert("death_sound", s.as_str());
        let s = self.eat_sound.to_string();
        component.insert("eat_sound", s.as_str());
        let s = self.hiss_sound.to_string();
        component.insert("hiss_sound", s.as_str());
        let s = self.hurt_sound.to_string();
        component.insert("hurt_sound", s.as_str());
        let s = self.purr_sound.to_string();
        component.insert("purr_sound", s.as_str());
        let s = self.purreow_sound.to_string();
        component.insert("purreow_sound", s.as_str());
        let s = self.stray_ambient_sound.to_string();
        component.insert("stray_ambient_sound", s.as_str());
        NbtTag::Compound(component)
    }
}

impl CatSoundVariant {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt());
        NbtTag::Compound(compound)
    }
}

pub type CatSoundVariantRef = &'static CatSoundVariant;

pub struct CatSoundVariantRegistry {
    cat_sound_variants_by_id: Vec<CatSoundVariantRef>,
    cat_sound_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CatSoundVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cat_sound_variants_by_id: Vec::new(),
            cat_sound_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, cat_sound_variant: CatSoundVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register cat sound variants after the registry has been frozen"
        );

        let id = self.cat_sound_variants_by_id.len();
        self.cat_sound_variants_by_key
            .insert(cat_sound_variant.key.clone(), id);
        self.cat_sound_variants_by_id.push(cat_sound_variant);
        id
    }

    /// Replaces a cat_sound_variant at a given index.
    /// Returns true if the cat_sound_variant was replaced and false if the cat_sound_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, cat_sound_variant: CatSoundVariantRef, id: usize) -> bool {
        if id >= self.cat_sound_variants_by_id.len() {
            return false;
        }
        self.cat_sound_variants_by_id[id] = cat_sound_variant;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<CatSoundVariantRef> {
        self.cat_sound_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, cat_sound_variant: CatSoundVariantRef) -> &usize {
        self.cat_sound_variants_by_key
            .get(&cat_sound_variant.key)
            .expect("cat sound variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<CatSoundVariantRef> {
        self.cat_sound_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, CatSoundVariantRef)> + '_ {
        self.cat_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cat_sound_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cat_sound_variants_by_id.is_empty()
    }
}

impl RegistryExt for CatSoundVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for CatSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
