use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

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

impl ToNbtTag for &ChickenAge {
    fn to_nbt_tag(self) -> NbtTag {
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

impl ToNbtTag for &ChickenSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt_tag());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt_tag());
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

    pub fn iter(&self) -> impl Iterator<Item = (usize, ChickenSoundVariantRef)> + '_ {
        self.chicken_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

crate::impl_registry!(
    ChickenSoundVariantRegistry,
    ChickenSoundVariant,
    chicken_sound_variants_by_id,
    chicken_sound_variants_by_key,
    chicken_sound_variants
);

impl Default for ChickenSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
