use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::{NbtCompound, NbtTag};
use steel_utils::Identifier;

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
impl ToNbtTag for &PigAge {
    fn to_nbt_tag(self) -> NbtTag {
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

impl ToNbtTag for &PigSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt_tag());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt_tag());
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
}

crate::impl_standard_methods!(
    PigSoundVariantRegistry,
    PigSoundVariantRef,
    pig_sound_variants_by_id,
    pig_sound_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    PigSoundVariantRegistry,
    PigSoundVariant,
    pig_sound_variants_by_id,
    pig_sound_variants_by_key,
    pig_sound_variants
);
