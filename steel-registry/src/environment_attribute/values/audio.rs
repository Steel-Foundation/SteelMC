//! Audio attribute values: vanilla `BackgroundMusic`, `sounds.Music`,
//! `AmbientSounds`, `AmbientMoodSettings`, and `AmbientAdditionsSettings`.

use std::borrow::Cow;

use steel_utils::{DowncastType, DowncastTypeKey};

use crate::sound_event::SoundEventRef;

/// A music track and the delay range between plays, as vanilla's `sounds.Music`.
#[derive(Debug, Clone, Copy)]
pub struct Music {
    pub sound: SoundEventRef,
    pub min_delay: i32,
    pub max_delay: i32,
    pub replace_current_music: bool,
}

/// Music played in an environment, as vanilla's `BackgroundMusic`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BackgroundMusic {
    pub default_music: Option<Music>,
    pub creative_music: Option<Music>,
    pub underwater_music: Option<Music>,
}

impl BackgroundMusic {
    pub const EMPTY: Self = Self {
        default_music: None,
        creative_music: None,
        underwater_music: None,
    };

    /// Picks the music for a player's situation, falling back to the default.
    #[must_use]
    pub const fn select(&self, is_creative: bool, is_underwater: bool) -> Option<Music> {
        if is_underwater && self.underwater_music.is_some() {
            return self.underwater_music;
        }
        if is_creative && self.creative_music.is_some() {
            return self.creative_music;
        }
        self.default_music
    }
}

/// Occasional cave mood sound, as vanilla's `AmbientMoodSettings`.
#[derive(Debug, Clone, Copy)]
pub struct AmbientMoodSettings {
    pub sound: SoundEventRef,
    pub tick_delay: i32,
    pub block_search_extent: i32,
    pub sound_position_offset: f64,
}

/// Randomly played additional ambient sound, as vanilla's `AmbientAdditionsSettings`.
#[derive(Debug, Clone, Copy)]
pub struct AmbientAdditionsSettings {
    pub sound: SoundEventRef,
    pub tick_chance: f64,
}

/// Ambient sounds of an environment, as vanilla's `AmbientSounds`.
#[derive(Debug, Clone)]
pub struct AmbientSounds {
    pub loop_sound: Option<SoundEventRef>,
    pub mood: Option<AmbientMoodSettings>,
    pub additions: Cow<'static, [AmbientAdditionsSettings]>,
}

impl AmbientSounds {
    pub const EMPTY: Self = Self {
        loop_sound: None,
        mood: None,
        additions: Cow::Borrowed(&[]),
    };
}

// SAFETY: This Steel-owned key uniquely identifies `BackgroundMusic` within the process.
unsafe impl DowncastType for BackgroundMusic {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:environment_attribute/background_music");
}

// SAFETY: This Steel-owned key uniquely identifies `AmbientSounds` within the process.
unsafe impl DowncastType for AmbientSounds {
    const TYPE_KEY: DowncastTypeKey =
        DowncastTypeKey::new("steel:environment_attribute/ambient_sounds");
}
