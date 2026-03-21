use rustc_hash::FxHashMap;
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

    pub fn register(&mut self, cow_sound_variant: CowSoundVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register cow sound variants after the registry has been frozen"
        );

        let id = self.cow_sound_variants_by_id.len();
        self.cow_sound_variants_by_key
            .insert(cow_sound_variant.key.clone(), id);
        self.cow_sound_variants_by_id.push(cow_sound_variant);
        id
    }

    /// Replaces a cow_sound_variant at a given index.
    /// Returns true if the cow_sound_variant was replaced and false if the cow_sound_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, cow_sound_variant: CowSoundVariantRef, id: usize) -> bool {
        if id >= self.cow_sound_variants_by_id.len() {
            return false;
        }
        self.cow_sound_variants_by_id[id] = cow_sound_variant;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, CowSoundVariantRef)> + '_ {
        self.cow_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

crate::impl_registry!(
    CowSoundVariantRegistry,
    CowSoundVariant,
    cow_sound_variants_by_id,
    cow_sound_variants_by_key,
    cow_sound_variants
);

impl Default for CowSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
