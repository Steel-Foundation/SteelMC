use rustc_hash::FxHashMap;
use steel_utils::Identifier;

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

    pub fn iter(&self) -> impl Iterator<Item = (usize, TimelineRef)> + '_ {
        self.timelines_by_id
            .iter()
            .enumerate()
            .map(|(id, &timeline)| (id, timeline))
    }
}

crate::impl_registry!(
    TimelineRegistry,
    Timeline,
    timelines_by_id,
    timelines_by_key,
    timelines
);
crate::impl_tagged_registry!(TimelineRegistry, timelines_by_key, "timeline");

impl Default for TimelineRegistry {
    fn default() -> Self {
        Self::new()
    }
}
