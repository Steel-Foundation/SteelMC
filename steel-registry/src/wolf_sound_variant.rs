use rustc_hash::FxHashMap;
use serde::Deserialize;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a set of sounds for a wolf variant from a data pack JSON file.
#[derive(Debug)]
pub struct WolfSoundVariant {
    pub key: Identifier,
    pub adult_sounds: WolfAge,
    pub baby_sounds: WolfAge,
}
#[derive(Deserialize, Debug)]
pub struct WolfAge {
    pub ambient_sound: Identifier,
    pub death_sound: Identifier,
    pub growl_sound: Identifier,
    pub hurt_sound: Identifier,
    pub pant_sound: Identifier,
    pub step_sound: Identifier,
    pub whine_sound: Identifier,
}

impl ToNbtTag for &WolfAge {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        let s = self.ambient_sound.to_string();
        compound.insert("ambient_sound", s.as_str());
        let s = self.death_sound.to_string();
        compound.insert("death_sound", s.as_str());
        let s = self.growl_sound.to_string();
        compound.insert("growl_sound", s.as_str());
        let s = self.hurt_sound.to_string();
        compound.insert("hurt_sound", s.as_str());
        let s = self.pant_sound.to_string();
        compound.insert("pant_sound", s.as_str());
        let s = self.step_sound.to_string();
        compound.insert("step_sound", s.as_str());
        let s = self.whine_sound.to_string();
        compound.insert("whine_sound", s.as_str());
        NbtTag::Compound(compound)
    }
}

impl ToNbtTag for &WolfSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt_tag());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt_tag());
        NbtTag::Compound(compound)
    }
}

pub type WolfSoundVariantRef = &'static WolfSoundVariant;

pub struct WolfSoundVariantRegistry {
    wolf_sound_variants_by_id: Vec<WolfSoundVariantRef>,
    wolf_sound_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl WolfSoundVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            wolf_sound_variants_by_id: Vec::new(),
            wolf_sound_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    WolfSoundVariantRegistry,
    WolfSoundVariantRef,
    wolf_sound_variants_by_id,
    wolf_sound_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    WolfSoundVariantRegistry,
    WolfSoundVariant,
    wolf_sound_variants_by_id,
    wolf_sound_variants_by_key,
    wolf_sound_variants
);
