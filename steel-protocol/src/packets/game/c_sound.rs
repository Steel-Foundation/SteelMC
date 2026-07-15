use glam::{DVec3, IVec3};
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SOUND;
use steel_registry::sound_event::SoundEventRef;

/// Sound source categories (matches vanilla `SoundSource` enum order).
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
    /// Returns the `VarInt` value for the enum.
    #[must_use]
    pub const fn as_varint(self) -> i32 {
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
    /// The holder-encoded sound event ID (`VarInt`).
    ///
    /// Vanilla reserves `0` for direct sound events, so registered sound events
    /// are encoded as `registry_id + 1`.
    #[write(as = VarInt)]
    pub sound_id: i32,
    /// The sound source category (`VarInt`).
    #[write(as = VarInt)]
    pub source: i32,
    /// X position multiplied by 8 (fixed-point).
    pub pos: IVec3,
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
    /// * `sound` - Sound event to play
    /// * `source` - Sound source category
    /// * `x`, `y`, `z` - Position in block coordinates (will be scaled by 8)
    /// * `volume` - Volume multiplier (1.0 = normal)
    /// * `pitch` - Pitch multiplier (1.0 = normal)
    /// * `seed` - Random seed for sound variations
    #[must_use]
    pub fn new(
        sound: SoundEventRef,
        source: SoundSource,
        pos: DVec3,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) -> Self {
        Self {
            sound_id: sound.packet_holder_id(),
            source: source.as_varint(),
            pos: IVec3::new(
                (pos.x * 8.0) as i32,
                (pos.y * 8.0) as i32,
                (pos.z * 8.0) as i32,
            ),
            volume,
            pitch,
            seed,
        }
    }

    /// Creates a block sound packet at the center of a block position.
    ///
    /// # Arguments
    /// * `sound` - Sound event to play
    /// * `pos` - Block position (will be centered at +0.5)
    /// * `volume` - Volume multiplier
    /// * `pitch` - Pitch multiplier
    /// * `seed` - Random seed
    #[must_use]
    pub fn block_sound(
        sound: SoundEventRef,
        pos: steel_utils::BlockPos,
        volume: f32,
        pitch: f32,
        seed: i64,
    ) -> Self {
        Self::new(
            sound,
            SoundSource::Blocks,
            pos.0.as_dvec3().map(|v| v + 0.5),
            volume,
            pitch,
            seed,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use steel_registry::{REGISTRY, Registry, RegistryEntry, sound_events};
    use steel_utils::BlockPos;

    use super::CSound;

    static INIT_REGISTRY: Once = Once::new();

    fn init_registry() {
        INIT_REGISTRY.call_once(|| {
            let mut registry = Registry::new_vanilla();
            registry.freeze();
            let _ = REGISTRY.init(registry);
        });
    }

    #[test]
    fn registered_sound_packet_uses_holder_id() {
        init_registry();

        let packet = CSound::block_sound(
            &sound_events::BLOCK_WOODEN_BUTTON_CLICK_ON,
            BlockPos::ZERO,
            1.0,
            1.0,
            0,
        );

        let expected_holder_id = sound_events::BLOCK_WOODEN_BUTTON_CLICK_ON.id() as i32 + 1;
        assert_eq!(
            sound_events::BLOCK_WOODEN_BUTTON_CLICK_ON.packet_holder_id(),
            expected_holder_id
        );
        assert_eq!(packet.sound_id, expected_holder_id);
    }
}
