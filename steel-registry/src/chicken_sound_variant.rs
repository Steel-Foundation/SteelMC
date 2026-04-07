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
}

crate::impl_standard_methods!(
    ChickenSoundVariantRegistry,
    ChickenSoundVariantRef,
    chicken_sound_variants_by_id,
    chicken_sound_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    ChickenSoundVariantRegistry,
    ChickenSoundVariant,
    chicken_sound_variants_by_id,
    chicken_sound_variants_by_key,
    chicken_sound_variants
);
