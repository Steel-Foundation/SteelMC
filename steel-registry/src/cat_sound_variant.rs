use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

/// Represents a set of sounds for a cat variant from a data pack JSON file.
#[derive(Debug)]
pub struct CatSoundVariant {
    pub key: Identifier,
    pub adult_sounds: CatAge,
    pub baby_sounds: CatAge,
}
#[derive(Debug)]
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

impl ToNbtTag for &CatAge {
    fn to_nbt_tag(self) -> NbtTag {
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

impl ToNbtTag for &CatSoundVariant {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtTag};
        let mut compound = NbtCompound::new();
        compound.insert("adult_sounds", self.adult_sounds.to_nbt_tag());
        compound.insert("baby_sounds", self.baby_sounds.to_nbt_tag());
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
}

crate::impl_standard_methods!(
    CatSoundVariantRegistry,
    CatSoundVariantRef,
    cat_sound_variants_by_id,
    cat_sound_variants_by_key,
    allows_registering
);

crate::impl_registry!(
    CatSoundVariantRegistry,
    CatSoundVariant,
    cat_sound_variants_by_id,
    cat_sound_variants_by_key,
    cat_sound_variants
);
