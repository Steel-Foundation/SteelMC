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
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl BannerPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            banner_patterns_by_id: Vec::new(),
            banner_patterns_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
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

    pub fn iter(&self) -> impl Iterator<Item = (usize, BannerPatternRef)> + '_ {
        self.banner_patterns_by_id
            .iter()
            .enumerate()
            .map(|(id, &pattern)| (id, pattern))
    }
}

impl Default for BannerPatternRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    BannerPatternRegistry,
    BannerPattern,
    banner_patterns_by_id,
    banner_patterns_by_key,
    banner_patterns
);

crate::impl_tagged_registry!(
    BannerPatternRegistry,
    banner_patterns_by_key,
    "banner pattern"
);
