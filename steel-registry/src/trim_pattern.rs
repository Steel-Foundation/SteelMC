use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

/// Represents an armor trim pattern definition from the data packs.
#[derive(Debug)]
pub struct TrimPattern {
    pub key: Identifier,
    pub asset_id: Identifier,
    pub description: TextComponent,
    pub decal: bool,
}

pub type TrimPatternRef = &'static TrimPattern;

pub struct TrimPatternRegistry {
    trim_patterns_by_id: Vec<TrimPatternRef>,
    trim_patterns_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl TrimPatternRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            trim_patterns_by_id: Vec::new(),
            trim_patterns_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, trim_pattern: TrimPatternRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register trim patterns after the registry has been frozen"
        );

        let id = self.trim_patterns_by_id.len();
        self.trim_patterns_by_key
            .insert(trim_pattern.key.clone(), id);
        self.trim_patterns_by_id.push(trim_pattern);
        id
    }

    /// Replaces a trim_pattern at a given index.
    /// Returns true if the trim_pattern was replaced and false if the trim_pattern wasn't replaced
    #[must_use]
    pub fn replace(&mut self, trim_pattern: TrimPatternRef, id: usize) -> bool {
        if id >= self.trim_patterns_by_id.len() {
            return false;
        }
        self.trim_patterns_by_id[id] = trim_pattern;
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, TrimPatternRef)> + '_ {
        self.trim_patterns_by_id
            .iter()
            .enumerate()
            .map(|(id, &pattern)| (id, pattern))
    }
}

impl Default for TrimPatternRegistry {
    fn default() -> Self {
        Self::new()
    }
}

crate::impl_registry!(
    TrimPatternRegistry,
    TrimPattern,
    trim_patterns_by_id,
    trim_patterns_by_key,
    trim_patterns
);
