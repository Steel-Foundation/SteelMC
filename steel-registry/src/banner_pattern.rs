use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Represents a banner pattern definition from a data pack JSON file.
#[derive(Debug)]
pub struct BannerPattern {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub translation_key: &'static str,
}

pub type BannerPatternRef = &'static BannerPattern;

pub struct BannerPatternRegistry {
    banner_patterns_by_id: Vec<BannerPatternRef>,
    banner_patterns_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl BannerPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            banner_patterns_by_id: Vec::new(),
            banner_patterns_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, banner_pattern: BannerPatternRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register banner patterns after the registry has been frozen"
        );

        let id = self.banner_patterns_by_id.len();
        self.banner_patterns_by_key
            .insert(banner_pattern.key.clone(), id);
        self.banner_patterns_by_id.push(banner_pattern);
        id
    }

    /// Replaces a banner_pattern at a given index.
    /// Returns true if the banner_pattern was replaced and false if the banner_pattern wasn't replaced
    #[must_use]
    pub fn replace(&mut self, banner_pattern: BannerPatternRef, id: usize) -> bool {
        if id >= self.banner_patterns_by_id.len() {
            return false;
        }
        self.banner_patterns_by_id[id] = banner_pattern;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<BannerPatternRef> {
        self.banner_patterns_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, banner_pattern: BannerPatternRef) -> &usize {
        self.banner_patterns_by_key
            .get(&banner_pattern.key)
            .expect("Banner pattern not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<BannerPatternRef> {
        self.banner_patterns_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, BannerPatternRef)> + '_ {
        self.banner_patterns_by_id
            .iter()
            .enumerate()
            .map(|(id, &pattern)| (id, pattern))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.banner_patterns_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.banner_patterns_by_id.is_empty()
    }
}

impl Default for BannerPatternRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RegistryExt for BannerPatternRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}
