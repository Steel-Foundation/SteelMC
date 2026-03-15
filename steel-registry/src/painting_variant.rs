use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

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

    /// Replaces a painting_variant at a given index.
    /// Returns true if the painting_variant was replaced and false if the painting_variant wasn't replaced
    #[must_use]
    pub fn replace(&mut self, painting_variant: PaintingVariantRef, id: usize) -> bool {
        if id >= self.painting_variants_by_id.len() {
            return false;
        }
        self.painting_variants_by_id[id] = painting_variant;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, PaintingVariantRef)> + '_ {
        self.painting_variants_by_id
            .iter()
            .enumerate()
            .map(|(id, &variant)| (id, variant))
    }
}

impl Default for PaintingVariantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    PaintingVariantRegistry,
    PaintingVariant,
    painting_variants_by_id,
    painting_variants_by_key,
    painting_variants
);
