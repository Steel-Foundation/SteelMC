use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;
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
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl PaintingVariantRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            painting_variants_by_id: Vec::new(),
            painting_variants_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
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

    pub fn register_tag(&mut self, tag: Identifier, keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifiers: Vec<Identifier> = keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifiers);
    }

    #[must_use]
    pub fn is_in_tag(&self, entry: PaintingVariantRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|entries| entries.contains(&entry.key))
    }

    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let entries = f(existing)
            .into_iter()
            .filter(|key| {
                let exists = self.painting_variants_by_key.contains_key(key);
                if !exists {
                    tracing::error!(
                        "painting variant {key} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), entries);
    }

    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<PaintingVariantRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = PaintingVariantRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
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
