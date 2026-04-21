use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a set of sounds for a cow variant from a data pack JSON file.
#[derive(Debug)]
pub struct CowSoundVariant {
    pub key: Identifier,
    pub ambient_sound: Identifier,
    pub death_sound: Identifier,
    pub hurt_sound: Identifier,
    pub step_sound: Identifier,
}

impl ToNbtTag for &CowSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        let s = self.ambient_sound.to_string();
        compound.insert("ambient_sound", s.as_str());
        let s = self.death_sound.to_string();
        compound.insert("death_sound", s.as_str());
        let s = self.hurt_sound.to_string();
        compound.insert("hurt_sound", s.as_str());
        let s = self.step_sound.to_string();
        compound.insert("step_sound", s.as_str());
        NbtTag::Compound(compound)
    }
}

crate::define_registry!(CowSoundVariantRegistry, CowSoundVariant, stem: cow_sound_variants);
