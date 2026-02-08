use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

/// Represents a timeline definition from a data pack JSON file.
#[derive(Debug)]
pub struct Timeline {
    pub key: Identifier,
}

pub type TimelineRef = &'static Timeline;

pub struct TimelineRegistry {
    timelines_by_id: Vec<TimelineRef>,
    timelines_by_key: FxHashMap<Identifier, usize>,
    tags: FxHashMap<Identifier, Vec<Identifier>>,
    allows_registering: bool,
}

impl TimelineRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            timelines_by_id: Vec::new(),
            timelines_by_key: FxHashMap::default(),
            tags: FxHashMap::default(),
            allows_registering: true,
        }
    }

    pub fn register(&mut self, timeline: TimelineRef) -> usize {
        assert!(
            self.allows_registering,
            "Cannot register timelines after the registry has been frozen"
        );

        let id = self.timelines_by_id.len();
        self.timelines_by_key.insert(timeline.key.clone(), id);
        self.timelines_by_id.push(timeline);
        id
    }

    /// Replaces a timelines at a given index.
    /// Returns true if the timeline was replaced and false if the timeline wasn't replaced
    #[must_use]
    pub fn replace(&mut self, timeline: TimelineRef, id: usize) -> bool {
        if id >= self.timelines_by_id.len() {
            return false;
        }
        self.timelines_by_id[id] = timeline;
        true
    }

    #[must_use]
    pub fn by_id(&self, id: usize) -> Option<TimelineRef> {
        self.timelines_by_id.get(id).copied()
    }

    #[must_use]
    pub fn get_id(&self, timeline: TimelineRef) -> &usize {
        self.timelines_by_key
            .get(&timeline.key)
            .expect("Timeline not found")
    }

    #[must_use]
    pub fn by_key(&self, key: &Identifier) -> Option<TimelineRef> {
        self.timelines_by_key
            .get(key)
            .and_then(|id| self.by_id(*id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, TimelineRef)> + '_ {
        self.timelines_by_id
            .iter()
            .enumerate()
            .map(|(id, &timeline)| (id, timeline))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.timelines_by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.timelines_by_id.is_empty()
    }

    // Tag-related methods

    /// Registers a tag with a list of timeline keys.
    /// Timeline keys that don't exist in the registry are silently skipped.
    pub fn register_tag(&mut self, tag: Identifier, timeline_keys: &[&'static str]) {
        assert!(
            self.allows_registering,
            "Cannot register tags after registry has been frozen"
        );

        let identifier: Vec<Identifier> = timeline_keys
            .iter()
            .filter_map(|key| {
                let ident = registry_vanilla_or_custom_tag(key);
                // Only include if the item actually exists
                self.by_key(&ident).map(|_| ident)
            })
            .collect();

        self.tags.insert(tag, identifier);
    }

    /// Checks if a fluid is in a given tag.
    #[must_use]
    pub fn is_in_tag(&self, timeline: TimelineRef, tag: &Identifier) -> bool {
        self.tags
            .get(tag)
            .is_some_and(|timelines| timelines.contains(&timeline.key))
    }

    /// Gives the access to all blocks to delete and add new entries
    pub fn modify_tag(
        &mut self,
        tag: &Identifier,
        f: impl FnOnce(Vec<Identifier>) -> Vec<Identifier>,
    ) {
        let existing = self.tags.remove(tag).unwrap_or_default();
        let timelines = f(existing)
            .into_iter()
            .filter(|timeline| {
                let exists = self.timelines_by_key.contains_key(timeline);
                if !exists {
                    tracing::error!(
                        "timeline {timeline} not found in registry, skipping from tag {tag}"
                    );
                }
                exists
            })
            .collect();
        self.tags.insert(tag.clone(), timelines);
    }

    /// Gets all timelines in a tag.
    #[must_use]
    pub fn get_tag(&self, tag: &Identifier) -> Option<Vec<TimelineRef>> {
        self.tags.get(tag).map(|idents| {
            idents
                .iter()
                .filter_map(|ident| self.by_key(ident))
                .collect()
        })
    }

    /// Iterates over all timelines in a tag.
    pub fn iter_tag(&self, tag: &Identifier) -> impl Iterator<Item = TimelineRef> + '_ {
        self.tags
            .get(tag)
            .into_iter()
            .flat_map(|v| v.iter().filter_map(|ident| self.by_key(ident)))
    }

    /// Returns an iterator over all tag keys.
    pub fn tag_keys(&self) -> impl Iterator<Item = &Identifier> {
        self.tags.keys()
    }
}

impl RegistryExt for TimelineRegistry {
    fn freeze(&mut self) {
        self.allows_registering = false;
    }
}

impl Default for TimelineRegistry {
    fn default() -> Self {
        Self::new()
    }
}
