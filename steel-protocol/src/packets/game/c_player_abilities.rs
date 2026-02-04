use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_ABILITIES;

/// Flags for player abilities bitfield.
/// These match vanilla Minecraft's ability flags.
pub mod ability_flags {
    pub const INVULNERABLE: u8 = 0x01;
    pub const FLYING: u8 = 0x02;
    pub const MAY_FLY: u8 = 0x04;
    pub const INSTABUILD: u8 = 0x08;
}

/// Sent by the server to update the player's abilities.
/// This tells the client whether the player can fly, is invulnerable, etc.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_PLAYER_ABILITIES)]
pub struct CPlayerAbilities {
    /// Bitfield of ability flags (invulnerable, flying, may_fly, instabuild)
    pub flags: u8,
    /// Flying speed (default 0.05)
    pub flying_speed: f32,
    /// Field of view modifier / walking speed (default 0.1)
    pub walking_speed: f32,
}

impl CPlayerAbilities {
    /// Default flying speed in vanilla Minecraft
    pub const DEFAULT_FLYING_SPEED: f32 = 0.05;
    /// Default walking speed in vanilla Minecraft
    pub const DEFAULT_WALKING_SPEED: f32 = 0.1;

    /// Creates abilities for survival mode
    pub fn survival() -> Self {
        Self {
            flags: 0,
            flying_speed: Self::DEFAULT_FLYING_SPEED,
            walking_speed: Self::DEFAULT_WALKING_SPEED,
        }
    }

    /// Creates abilities for creative mode
    pub fn creative() -> Self {
        Self {
            flags: ability_flags::INVULNERABLE | ability_flags::MAY_FLY | ability_flags::INSTABUILD,
            flying_speed: Self::DEFAULT_FLYING_SPEED,
            walking_speed: Self::DEFAULT_WALKING_SPEED,
        }
    }

    /// Creates abilities for adventure mode
    pub fn adventure() -> Self {
        Self {
            flags: 0,
            flying_speed: Self::DEFAULT_FLYING_SPEED,
            walking_speed: Self::DEFAULT_WALKING_SPEED,
        }
    }

    /// Creates abilities for spectator mode
    pub fn spectator() -> Self {
        Self {
            // Spectators: invulnerable, can fly, and are currently flying
            flags: ability_flags::INVULNERABLE | ability_flags::MAY_FLY | ability_flags::FLYING,
            flying_speed: Self::DEFAULT_FLYING_SPEED,
            walking_speed: Self::DEFAULT_WALKING_SPEED,
        }
    }
}
