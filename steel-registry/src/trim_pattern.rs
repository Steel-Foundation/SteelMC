use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use text_components::TextComponent;

use crate::RegistryExt;

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

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<TrimPatternRef> {
        self.trim_patterns_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, trim_pattern: TrimPatternRef) -> &usize {
        self.trim_patterns_by_key
            .get(&trim_pattern.key)
            .expect("Trim pattern not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<TrimPatternRef> {
        self.trim_patterns_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, TrimPatternRef)> + '_ {
        self.trim_patterns_by_id
            .iter()
            .enumerate()
            .map(|(id, &pattern)| (id, pattern))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.trim_patterns_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.trim_patterns_by_id.is_empty()
    }
}

impl RegistryExt for TrimPatternRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for TrimPatternRegistry {
    fn default() -> Self {
        Self::new()
    }
}
