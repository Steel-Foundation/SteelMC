use rustc_hash::FxHashMap;
use steel_utils::Identifier;

use crate::RegistryExt;

/// Represents a set of sounds for a wolf variant from a data pack JSON file.
#[derive(Debug)]
pub struct WolfSoundVariant {
    pub key: Identifier,
    pub ambient_sound: Identifier,
    pub death_sound: Identifier,
    pub growl_sound: Identifier,
    pub hurt_sound: Identifier,
    pub pant_sound: Identifier,
    pub whine_sound: Identifier,
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

    pub fn register(&mut self, wolf_sound_variant: WolfSoundVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register wolf sound variants after the registry has been frozen"
        );

        let id = self.wolf_sound_variants_by_id.len();
        self.wolf_sound_variants_by_key
            .insert(wolf_sound_variant.key.clone(), id);
        self.wolf_sound_variants_by_id.push(wolf_sound_variant);
        id
    }

    /// Replaces a wolf_sound_variant at a given index.
    /// Returns true if the wolf_sound_variant was replaced and false if the wolf_sound_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, wolf_sound_variant: WolfSoundVariantRef, id: usize) -> bool {
        if id >= self.wolf_sound_variants_by_id.len() {
            return false;
        }
        self.wolf_sound_variants_by_id[id] = wolf_sound_variant;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<WolfSoundVariantRef> {
        self.wolf_sound_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, wolf_sound_variant: WolfSoundVariantRef) -> &usize {
        self.wolf_sound_variants_by_key
            .get(&wolf_sound_variant.key)
            .expect("Wolf sound variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<WolfSoundVariantRef> {
        self.wolf_sound_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, WolfSoundVariantRef)> + '_ {
        self.wolf_sound_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.wolf_sound_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wolf_sound_variants_by_id.is_empty()
    }
}

impl RegistryExt for WolfSoundVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for WolfSoundVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
