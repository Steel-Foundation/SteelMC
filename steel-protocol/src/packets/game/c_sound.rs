use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SOUND;

/// Sound source categories (matches vanilla SoundSource enum order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SoundSource {
    Master = 0,
    Music = 1,
    Records = 2,
    Weather = 3,
    Blocks = 4,
    Hostile = 5,
    Neutral = 6,
    Players = 7,
    Ambient = 8,
    Voice = 9,
    Ui = 10,
}

impl SoundSource {
    /// Returns the VarInt value for the enum.
    #[must_use]
    pub fn as_varint(self) -> i32 {
        self as i32
    }
}

/// Sent to play a sound effect at a specific position.
///
/// The position is encoded at 8x precision (divide by 8 to get actual block coordinates).
/// This allows sub-block positioning for more accurate sound placement.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SOUND)]
pub struct CSound {
    /// The sound event registry ID (VarInt).
    /// Use `steel_registry::sound_events` for sound constants.
    #[write(as = VarInt)]
    pub sound_id: i32,
    /// The sound source category (VarInt).
    #[write(as = VarInt)]
    pub source: i32,
    /// X position multiplied by 8 (fixed-point).
    pub x: i32,
    /// Y position multiplied by 8 (fixed-point).
    pub y: i32,
    /// Z position multiplied by 8 (fixed-point).
    pub z: i32,
    /// Volume (1.0 = normal).
    pub volume: f32,
    /// Pitch (1.0 = normal).
    pub pitch: f32,
    /// Random seed for sound variations.
    pub seed: i64,
}

impl CSound {
    /// Creates a new sound packet.
    ///
    /// # Arguments
    /// * `sound_id` - Sound event registry ID
    /// * `source` - Sound source category
    /// * `x`, `y`, `z` - Position in block coordinates (will be scaled by 8)
    /// * `volume` - Volume multiplier (1.0 = normal)
    /// * `pitch` - Pitch multiplier (1.0 = normal)
    /// * `seed` - Random seed for sound variations
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sound_id: i32,
        source: SoundSource,
        x: f64,
        y: f64,
        z: f64,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) -> Self {
        Self {
            sound_id,
            source: source.as_varint(),
            x: (x * 8.0) as i32,
            y: (y * 8.0) as i32,
            z: (z * 8.0) as i32,
            volume,
            pitch,
            seed,
        }
    }

    /// Creates a block sound packet at the center of a block position.
    ///
    /// # Arguments
    /// * `sound_id` - Sound event registry ID
    /// * `pos` - Block position (will be centered at +0.5)
    /// * `volume` - Volume multiplier
    /// * `pitch` - Pitch multiplier
    /// * `seed` - Random seed
    #[must_use]
    pub fn block_sound(
        sound_id: i32,
        pos: steel_utils::BlockPos,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) -> Self {
        Self::new(
            sound_id,
            SoundSource::Blocks,
            f64::from(pos.x()) + 0.5,
            f64::from(pos.y()) + 0.5,
            f64::from(pos.z()) + 0.5,
            volume,
            pitch,
            seed,
        )
    }
}
