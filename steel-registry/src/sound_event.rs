use rustc_hash::FxHashMap;
use steel_utils::Identifier;

/// Built-in sound event registry entry used by sound packets and data-driven audio refs.
#[derive(Debug, PartialEq)]
pub struct SoundEvent {
    pub key: Identifier,
    pub sound_id: Identifier,
    pub fixed_range: Option<f32>,
}

impl SoundEvent {
    /// Vanilla `SoundEvent.getRange`.
    #[must_use]
    pub fn range(&self, volume: f32) -> f32 {
        self.fixed_range
            .unwrap_or(if volume > 1.0 { 16.0 * volume } else { 16.0 })
    }

    /// Returns the VarInt payload used by vanilla holder-based sound packets.
    #[must_use]
    pub fn packet_holder_id(&self) -> i32 {
        let id = crate::RegistryEntry::id(self);
        assert!(
            id < i32::MAX as usize,
            "sound event registry id exceeds protocol VarInt range"
        );
        id as i32 + 1
    }
}

pub type SoundEventRef = &'static SoundEvent;

pub struct SoundEventRegistry {
    sound_events_by_id: Vec<SoundEventRef>,
    sound_events_by_key: FxHashMap<Identifier, usize>,
    allows_registering: bool,
}

impl SoundEventRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sound_events_by_id: Vec::new(),
            sound_events_by_key: FxHashMap::default(),
            allows_registering: true,
        }
    }
}

crate::impl_standard_methods!(
    SoundEventRegistry,
    SoundEventRef,
    sound_events_by_id,
    sound_events_by_key,
    allows_registering
);

crate::impl_registry!(
    SoundEventRegistry,
    SoundEvent,
    sound_events_by_id,
    sound_events_by_key,
    sound_events
);
