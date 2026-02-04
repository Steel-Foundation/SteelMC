use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::RegistryExt;

/// Represents a painting variant definition from a data pack JSON file.
#[derive(Debug)]
pub struct PaintingVariant {
    pub key: Identifier,
    pub width: i32,
    pub height: i32,
    pub asset_id: Identifier,
    pub title: Option<TextComponent>,
    pub author: Option<TextComponent>,
}

pub type PaintingVariantRef = &'static PaintingVariant;

pub struct PaintingVariantRegistry {
    painting_variants_by_id: Vec<PaintingVariantRef>,
    painting_variants_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl PaintingVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            painting_variants_by_id: Vec::new(),
            painting_variants_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, painting_variant: PaintingVariantRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register painting variants after the registry has been frozen"
        );

        let id = self.painting_variants_by_id.len();
        self.painting_variants_by_key
            .insert(painting_variant.key.clone(), id);
        self.painting_variants_by_id.push(painting_variant);
        id
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<PaintingVariantRef> {
        self.painting_variants_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, painting_variant: PaintingVariantRef) -> &usize {
        self.painting_variants_by_key
            .get(&painting_variant.key)
            .expect("Painting variant not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<PaintingVariantRef> {
        self.painting_variants_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PaintingVariantRef)> + '_ {
        self.painting_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.painting_variants_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.painting_variants_by_id.is_empty()
    }
}

impl RegistryExt for PaintingVariantRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for PaintingVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}
