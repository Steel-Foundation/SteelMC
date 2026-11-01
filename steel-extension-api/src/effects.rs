//! Particles, sounds, potion effects, experience and movement speeds.
//!
//! [`Particle`] describes one vanilla particle burst. Options are taken from the fields the
//! particle type uses: `color`/`scale` for `dust`, `color`/`to_color`/`scale` for
//! `dust_color_transition`, `block` for `block`, `falling_dust`, `dust_pillar` and
//! `block_marker`, `color` for `entity_effect` and other color particles. Like
//! `/particle`, `count = 0` sends a single particle whose `spread` is its velocity; for
//! `note` this is how the color is chosen ([`Particle::note`]).
//!
//! Sounds accept any sound event key, including resource-pack sounds.

use crate::Position;
use stabby::string::String as ApiString;

/// One particle burst.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq)]
pub struct Particle {
    /// Particle type key, such as `minecraft:flame`.
    pub key: ApiString,
    /// Block state for block particles, such as `minecraft:gold_block`.
    pub block: ApiString,
    /// Center.
    pub position: Position,
    /// Gaussian spread per axis (or velocity when `count` is 0).
    pub spread: Position,
    /// Speed.
    pub speed: f64,
    /// ARGB color for dust and color particles.
    pub color: u32,
    /// ARGB end color for `dust_color_transition`.
    pub to_color: u32,
    /// Number of particles.
    pub count: i32,
    /// Dust scale (0.01 to 4).
    pub scale: f32,
    /// Shows to players up to 512 blocks away and past the client's particle setting.
    pub force: bool,
}

impl Particle {
    /// `count` particles of `key` around `position`.
    #[must_use]
    pub fn new(key: &str, position: Position, count: i32) -> Self {
        Self {
            key: key.into(),
            block: ApiString::default(),
            position,
            spread: Position {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            speed: 0.0,
            color: 0xFFFF_FFFF,
            to_color: 0xFFFF_FFFF,
            count,
            scale: 1.0,
            force: false,
        }
    }

    /// A single `note` particle; `pitch` 0–24 selects its color like a note block.
    #[must_use]
    pub fn note(position: Position, pitch: u8) -> Self {
        Self::new("minecraft:note", position, 0)
            .spread(f64::from(pitch.min(24)) / 24.0, 0.0, 0.0)
            .speed(1.0)
    }

    /// Sets the spread per axis.
    #[must_use]
    pub const fn spread(mut self, x: f64, y: f64, z: f64) -> Self {
        self.spread = Position { x, y, z };
        self
    }

    /// Sets the speed.
    #[must_use]
    pub const fn speed(mut self, speed: f64) -> Self {
        self.speed = speed;
        self
    }

    /// Sets the RGB color and scale (for dust).
    #[must_use]
    pub const fn dust(mut self, rgb: u32, scale: f32) -> Self {
        self.color = 0xFF00_0000 | rgb;
        self.scale = scale;
        self
    }

    /// Sets the end RGB color (for `dust_color_transition`).
    #[must_use]
    pub const fn fade_to(mut self, rgb: u32) -> Self {
        self.to_color = 0xFF00_0000 | rgb;
        self
    }

    /// Sets the block state (for block particles).
    #[must_use]
    pub fn block(mut self, state: &str) -> Self {
        self.block = state.into();
        self
    }

    /// Sends past the normal range and particle setting.
    #[must_use]
    pub const fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }
}

/// Vanilla sound category (volume slider).
#[stabby::stabby]
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundCategory {
    /// Master volume.
    Master,
    /// Music.
    Music,
    /// Jukebox and note blocks.
    Records,
    /// Weather.
    Weather,
    /// Blocks.
    Blocks,
    /// Hostile creatures.
    Hostile,
    /// Friendly creatures.
    Neutral,
    /// Players.
    Players,
    /// Ambient environment.
    Ambient,
    /// Voice and speech.
    Voice,
    /// User interface.
    Ui,
}

/// A potion effect to apply.
#[stabby::stabby]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    /// Effect key, such as `minecraft:haste`.
    pub key: ApiString,
    /// Duration in ticks; -1 for infinite.
    pub duration: i32,
    /// Amplifier (0 is level I).
    pub amplifier: i32,
    /// Beacon-style translucent particles.
    pub ambient: bool,
    /// Shows particles.
    pub particles: bool,
    /// Shows the HUD icon.
    pub icon: bool,
}

impl Effect {
    /// An effect with visible particles and icon.
    #[must_use]
    pub fn new(key: &str, duration: i32, amplifier: i32) -> Self {
        Self {
            key: key.into(),
            duration,
            amplifier,
            ambient: false,
            particles: true,
            icon: true,
        }
    }

    /// Hides the particles (the icon stays).
    #[must_use]
    pub const fn hide_particles(mut self) -> Self {
        self.particles = false;
        self
    }
}

#[stabby::host_api]
/// Effect bindings on `World` and `Server`.
pub mod api {
    use super::{Effect, Particle, SoundCategory};
    use crate::{ApiResult, PlayerId, Position, Server, World};
    use stabby::str::Str;
    #[cfg(feature = "host")]
    use stabby::string::String as ApiString;

    #[cfg(feature = "host")]
    use crate::host::effects as host;

    impl<'a> World<'a> {
        /// Sends a particle burst to nearby players; returns how many received it.
        pub fn spawn_particle(&self, particle: Particle) -> ApiResult<i32> {
            host::spawn_particle(self.host(), &particle)
                .map_err(ApiString::from)
                .into()
        }

        /// Plays a sound at a position for nearby players.
        pub fn play_sound(
            &self,
            sound: Str<'_>,
            position: Position,
            category: SoundCategory,
            volume: f32,
            pitch: f32,
        ) -> ApiResult<()> {
            host::play_sound(
                self.host(),
                sound.as_str(),
                position,
                category,
                volume,
                pitch,
            )
            .map_err(ApiString::from)
            .into()
        }
    }

    impl<'a> Server<'a> {
        /// Plays a sound only this player hears, at their position.
        pub fn play_sound_to(
            &self,
            player: PlayerId,
            sound: Str<'_>,
            category: SoundCategory,
            volume: f32,
            pitch: f32,
        ) -> ApiResult<()> {
            host::play_sound_to(self.host(), player, sound.as_str(), category, volume, pitch)
                .map_err(ApiString::from)
                .into()
        }

        /// Adds or upgrades a potion effect; returns whether it changed.
        pub fn add_effect(&self, player: PlayerId, effect: Effect) -> ApiResult<bool> {
            host::add_effect(self.host(), player, &effect)
                .map_err(ApiString::from)
                .into()
        }

        /// Removes a potion effect; returns whether it was active.
        pub fn remove_effect(&self, player: PlayerId, effect: Str<'_>) -> ApiResult<bool> {
            host::remove_effect(self.host(), player, effect.as_str())
                .map_err(ApiString::from)
                .into()
        }

        /// Gives experience points (negative removes), firing the experience change event.
        pub fn give_experience(&self, player: PlayerId, points: i32) -> ApiResult<()> {
            host::give_experience(self.host(), player, points)
                .map_err(ApiString::from)
                .into()
        }

        /// Gives experience levels (negative removes).
        pub fn give_levels(&self, player: PlayerId, levels: i32) -> ApiResult<()> {
            host::give_levels(self.host(), player, levels)
                .map_err(ApiString::from)
                .into()
        }

        /// Sets the walking speed (vanilla default 0.1).
        pub fn set_walk_speed(&self, player: PlayerId, speed: f32) -> ApiResult<()> {
            host::set_walk_speed(self.host(), player, speed)
                .map_err(ApiString::from)
                .into()
        }

        /// Sets the flying speed (vanilla default 0.05).
        pub fn set_fly_speed(&self, player: PlayerId, speed: f32) -> ApiResult<()> {
            host::set_fly_speed(self.host(), player, speed)
                .map_err(ApiString::from)
                .into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_particles_encode_pitch_as_velocity() {
        let origin = Position {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let note = Particle::note(origin, 12);
        assert_eq!(note.count, 0);
        assert!((note.spread.x - 0.5).abs() < f64::EPSILON);
        assert!((Particle::note(origin, 99).spread.x - 1.0).abs() < f64::EPSILON);
        assert_eq!(
            Particle::new("x", origin, 1).dust(0x12_3456, 2.0).color,
            0xFF12_3456
        );
    }
}
