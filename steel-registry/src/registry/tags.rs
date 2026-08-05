use rustc_hash::{FxHashMap, FxHashSet};
use steel_utils::Identifier;

#[derive(Debug, Default)]
pub struct RegistryTags {
    tags: FxHashMap<Identifier, RegistryTag>,
}

#[derive(Debug)]
struct RegistryTag {
    ordered_keys: Vec<&'static Identifier>,
    member_keys: FxHashSet<&'static Identifier>,
}

impl RegistryTag {
    fn new(ordered_keys: Vec<&'static Identifier>) -> Self {
        let member_keys = ordered_keys.iter().copied().collect();
        Self {
            ordered_keys,
            member_keys,
        }
    }
}

impl RegistryTags {
    #[doc(hidden)]
    pub fn insert(&mut self, tag: Identifier, ordered_keys: Vec<&'static Identifier>) {
        self.tags.insert(tag, RegistryTag::new(ordered_keys));
    }

    #[doc(hidden)]
    pub fn remove(&mut self, tag: &Identifier) -> Option<Vec<&'static Identifier>> {
        self.tags.remove(tag).map(|tag| tag.ordered_keys)
    }

    #[doc(hidden)]
    #[must_use]
    pub fn contains(&self, tag: &Identifier, entry_key: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|entries| entries.member_keys.contains(entry_key))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn get(&self, tag: &Identifier) -> Option<&[&'static Identifier]> {
        self.tags
            .get(tag)
            .map(|entries| entries.ordered_keys.as_slice())
    }

    #[doc(hidden)]
    pub fn keys(&self) -> impl Iterator<Item = &Identifier> + '_ {
        self.tags.keys()
    }
}
