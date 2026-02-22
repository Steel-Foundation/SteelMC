use crate::RegistryExt;
use rustc_hash::FxHashMap;
use steel_utils::Identifier;
use steel_utils::registry::registry_vanilla_or_custom_tag;

#[derive(Debug, Clone)]
pub enum KeyframeValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(&'static str),
}

#[derive(Debug, Clone)]
pub enum Ease {
    Named(&'static str),
    CubicBezier([f32; 4]),
}

#[derive(Debug)]
pub struct Keyframe {
    pub ticks: i64,
    pub value: KeyframeValue,
}

#[derive(Debug)]
pub struct Track {
    pub name: &'static str,
    pub ease: Option<Ease>,
    pub modifier: Option<&'static str>,
    pub keyframes: &'static [Keyframe],
}

/// show_in_commands: None  = serialize marker as plain NbtTag::Int(ticks as i32)
///                  Some(b)= serialize as compound {ticks: int, show_in_commands: byte}
#[derive(Debug)]
pub struct TimeMarker {
    pub name: &'static str,
    pub ticks: i64,
    pub show_in_commands: Option<bool>,
}

/// Represents a timeline definition from a data pack JSON file.
#[derive(Debug)]
pub struct Timeline {
    pub key: Identifier,
    pub clock: Option<Identifier>,
    pub period_ticks: Option<i64>,
    pub tracks: &'static [Track],
    pub time_markers: &'static [TimeMarker],
}

impl Timeline {
    pub fn to_nbt(&self) -> simdnbt::owned::NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();

        if let Some(clock) = &self.clock {
            compound.insert("clock", clock.to_string().as_str());
        }
        if let Some(pt) = self.period_ticks {
            compound.insert("period_ticks", pt); // i64 → Long
        }

        let mut tracks_compound = NbtCompound::new();
        for track in self.tracks {
            let mut tc = NbtCompound::new();
            if let Some(ease) = &track.ease {
                match ease {
                    Ease::Named(s) => tc.insert("ease", *s),
                    Ease::CubicBezier(coeffs) => {
                        let mut ec = NbtCompound::new();
                        ec.insert(
                            "cubic_bezier",
                            NbtTag::List(NbtList::Float(coeffs.to_vec())),
                        );
                        tc.insert("ease", NbtTag::Compound(ec));
                    }
                }
            }
            if let Some(modifier) = track.modifier {
                tc.insert("modifier", modifier);
            }
            let kf_compounds: Vec<NbtCompound> = track
                .keyframes
                .iter()
                .map(|kf| {
                    let mut kc = NbtCompound::new();
                    kc.insert("ticks", kf.ticks); // i64 → Long
                    match &kf.value {
                        KeyframeValue::Float(f) => kc.insert("value", *f),
                        KeyframeValue::Int(i) => kc.insert("value", *i),
                        KeyframeValue::Bool(b) => kc.insert("value", if *b { 1i8 } else { 0i8 }),
                        KeyframeValue::String(s) => kc.insert("value", *s),
                    }
                    kc
                })
                .collect();
            tc.insert("keyframes", NbtTag::List(NbtList::Compound(kf_compounds)));
            tracks_compound.insert(track.name, NbtTag::Compound(tc));
        }
        compound.insert("tracks", NbtTag::Compound(tracks_compound));

        if !self.time_markers.is_empty() {
            let mut mc = NbtCompound::new();
            for marker in self.time_markers {
                match marker.show_in_commands {
                    None => mc.insert(marker.name, marker.ticks as i32),
                    Some(show) => {
                        let mut mcc = NbtCompound::new();
                        mcc.insert("ticks", marker.ticks as i32);
                        mcc.insert("show_in_commands", if show { 1i8 } else { 0i8 });
                        mc.insert(marker.name, NbtTag::Compound(mcc));
                    }
                }
            }
            compound.insert("time_markers", NbtTag::Compound(mc));
        }

        NbtTag::Compound(compound)
    }
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
