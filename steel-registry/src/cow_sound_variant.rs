use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

use crate::sound_event::SoundEventRef;

/// Represents a set of sounds for a cow variant from a data pack JSON file.
#[derive(Debug)]
pub struct CowSoundVariant {
    pub key: Identifier,
    pub ambient_sound: SoundEventRef,
    pub death_sound: SoundEventRef,
    pub hurt_sound: SoundEventRef,
    pub step_sound: SoundEventRef,
}

impl ToNbtTag for &CowSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        let s = self.ambient_sound.key.to_string();
        compound.insert("ambient_sound", s.as_str());
        let s = self.death_sound.key.to_string();
        compound.insert("death_sound", s.as_str());
        let s = self.hurt_sound.key.to_string();
        compound.insert("hurt_sound", s.as_str());
        let s = self.step_sound.key.to_string();
        compound.insert("step_sound", s.as_str());
        NbtTag::Compound(compound)
    }
}

pub type CowSoundVariantRef = &'static CowSoundVariant;

pub struct CowSoundVariantRegistry {
    cow_sound_variants_by_id: Vec<CowSoundVariantRef>,
    cow_sound_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl CowSoundVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cow_sound_variants_by_id: Vec::new(),
            cow_sound_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    CowSoundVariantRegistry,
    CowSoundVariantRef,
    cow_sound_variants_by_id,
    cow_sound_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    CowSoundVariantRegistry,
    CowSoundVariant,
    cow_sound_variants_by_id,
    cow_sound_variants_by_key,
    cow_sound_variants
);
