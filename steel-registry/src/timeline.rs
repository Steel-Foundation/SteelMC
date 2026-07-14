use rustc_hash::FxHashMap;
use simdnbt::ToNbtTag;
use simdnbt::owned::NbtTag;
use steel_utils::Identifier;

use crate::world_clock::WorldClockRef;

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

/// `show_in_commands`: None  = serialize marker as plain `NbtTag::Int(ticks` as i32)
///                  Some(b)= serialize as compound {ticks: int, `show_in_commands`: byte}
#[derive(Debug)]
pub struct TimeMarker {
    pub key: Identifier,
    pub ticks: i32,
    pub show_in_commands: Option<bool>,
}

impl TimeMarker {
    /// Resolves the next occurrence using vanilla's strictly-forward periodic behavior.
    #[must_use]
    pub fn resolve_time_to_move_to(&self, total_ticks: i64, period_ticks: Option<i32>) -> i64 {
        let Some(period_ticks) = period_ticks else {
            return i64::from(self.ticks);
        };
        let period_ticks = i64::from(period_ticks);
        let duration = i64::from(self.ticks) - total_ticks % period_ticks;
        total_ticks.wrapping_add(if duration > 0 {
            duration
        } else {
            period_ticks + duration
        })
    }
}

/// Represents a timeline definition from a data pack JSON file.
#[derive(Debug)]
pub struct Timeline {
    pub key: Identifier,
    pub clock: WorldClockRef,
    pub period_ticks: Option<i32>,
    pub tracks: &'static [Track],
    pub time_markers: &'static [TimeMarker],
}

impl Timeline {
    /// Returns the current tick inside this timeline's period.
    #[must_use]
    pub fn current_ticks(&self, total_ticks: i64) -> i64 {
        self.period_ticks
            .map_or(total_ticks, |period| total_ticks % i64::from(period))
    }

    /// Returns the completed period count using vanilla's narrowing conversion.
    #[must_use]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "vanilla narrows the completed long period count to a signed int"
    )]
    pub fn period_count(&self, total_ticks: i64) -> i32 {
        self.period_ticks
            .map_or(0, |period| (total_ticks / i64::from(period)) as i32)
    }
}

impl ToNbtTag for &Timeline {
    fn to_nbt_tag(self) -> NbtTag {
        use simdnbt::owned::{NbtCompound, NbtList, NbtTag};
        let mut compound = NbtCompound::new();

        compound.insert("clock", self.clock.key.to_string().as_str());
        if let Some(pt) = self.period_ticks {
            compound.insert("period_ticks", pt);
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
                        KeyframeValue::Bool(b) => kc.insert("value", i8::from(*b)),
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
                let key = marker.key.to_string();
                match marker.show_in_commands {
                    None => mc.insert(key.as_str(), marker.ticks),
                    Some(show) => {
                        let mut mcc = NbtCompound::new();
                        mcc.insert("ticks", marker.ticks);
                        mcc.insert("show_in_commands", i8::from(show));
                        mc.insert(key.as_str(), NbtTag::Compound(mcc));
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
}

crate::impl_standard_methods!(
    TimelineRegistry,
    TimelineRef,
    timelines_by_id,
    timelines_by_key,
    allows_registering
);

crate::impl_registry!(
    TimelineRegistry,
    Timeline,
    timelines_by_id,
    timelines_by_key,
    timelines
);
crate::impl_tagged_registry!(TimelineRegistry, timelines_by_key, "timeline");

#[cfg(test)]
mod tests {
    use crate::vanilla_timelines::{DAY, EARLY_GAME};

    #[test]
    fn repeating_timeline_reports_current_tick_and_completed_periods() {
        assert_eq!(DAY.current_ticks(25_000), 1_000);
        assert_eq!(DAY.period_count(25_000), 1);
    }

    #[test]
    fn non_repeating_timeline_uses_absolute_ticks_and_zero_periods() {
        assert_eq!(EARLY_GAME.current_ticks(25_000), 25_000);
        assert_eq!(EARLY_GAME.period_count(25_000), 0);
    }
}
