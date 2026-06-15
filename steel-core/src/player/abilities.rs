//! Player abilities (flight, invulnerability, etc.)

use steel_protocol::packets::game::{CPlayerAbilities, SPlayerAbilities, ability_flags};
use steel_utils::types::GameType;

use crate::player::Player;

/// Default flying speed in vanilla Minecraft
pub const DEFAULT_FLYING_SPEED: f32 = 0.05;
/// Default walking speed in vanilla Minecraft
pub const DEFAULT_WALKING_SPEED: f32 = 0.1;

/// Player abilities that control flight, invulnerability, and other special states.
/// This mirrors vanilla's `Abilities` class.
#[derive(Debug, Clone)]
pub struct Abilities {
    /// Whether the player is invulnerable to damage
    pub invulnerable: bool,
    /// Whether the player is currently flying (creative/spectator flight)
    pub flying: bool,
    /// Whether the player is allowed to fly
    pub may_fly: bool,
    /// Whether the player can instantly break blocks (creative mode)
    pub instabuild: bool,
    /// Whether the player can place/break blocks
    pub may_build: bool,
    /// Flying speed (default 0.05)
    pub flying_speed: f32,
    /// Walking speed (default 0.1)
    pub walking_speed: f32,
}

impl Default for Abilities {
    fn default() -> Self {
        Self {
            invulnerable: false,
            flying: false,
            may_fly: false,
            instabuild: false,
            may_build: true,
            flying_speed: DEFAULT_FLYING_SPEED,
            walking_speed: DEFAULT_WALKING_SPEED,
        }
    }
}

impl Abilities {
    /// Creates default abilities for survival mode
    #[must_use]
    pub fn survival() -> Self {
        Self::default()
    }

    /// Creates abilities for creative mode
    #[must_use]
    pub fn creative() -> Self {
        Self {
            invulnerable: true,
            flying: false,
            may_fly: true,
            instabuild: true,
            may_build: true,
            ..Self::default()
        }
    }

    /// Creates abilities for adventure mode
    #[must_use]
    pub fn adventure() -> Self {
        Self {
            may_build: false,
            ..Self::default()
        }
    }

    /// Creates abilities for spectator mode
    #[must_use]
    pub fn spectator() -> Self {
        Self {
            invulnerable: true,
            flying: true,
            may_fly: true,
            instabuild: false,
            may_build: false,
            ..Self::default()
        }
    }

    /// Updates abilities based on the given game mode.
    /// This mirrors vanilla's `GameType.updatePlayerAbilities()`.
    pub const fn update_for_game_mode(&mut self, game_mode: GameType) {
        match game_mode {
            GameType::Survival => {
                self.invulnerable = false;
                self.may_fly = false;
                self.instabuild = false;
                self.flying = false;
                self.may_build = true;
            }
            GameType::Creative => {
                self.invulnerable = true;
                self.may_fly = true;
                self.instabuild = true;
                // flying state is preserved
                self.may_build = true;
            }
            GameType::Adventure => {
                self.invulnerable = false;
                self.may_fly = false;
                self.instabuild = false;
                self.flying = false;
                self.may_build = false;
            }
            GameType::Spectator => {
                self.invulnerable = true;
                self.may_fly = true;
                self.instabuild = false;
                self.flying = true;
                self.may_build = false;
            }
        }
    }

    /// Converts abilities to a clientbound packet
    #[must_use]
    pub const fn to_packet(&self) -> CPlayerAbilities {
        let mut flags: u8 = 0;

        if self.invulnerable {
            flags |= ability_flags::INVULNERABLE;
        }
        if self.flying {
            flags |= ability_flags::FLYING;
        }
        if self.may_fly {
            flags |= ability_flags::MAY_FLY;
        }
        if self.instabuild {
            flags |= ability_flags::INSTABUILD;
        }

        CPlayerAbilities {
            flags,
            flying_speed: self.flying_speed,
            walking_speed: self.walking_speed,
        }
    }
}

impl Player {
    /// Sends the player abilities packet to the client.
    /// This tells the client about flight, invulnerability, speeds, etc.
    pub fn send_abilities(&self) {
        let packet = self.abilities.lock().to_packet();
        self.send_packet(packet);
    }

    /// Returns true if the player is flying (creative/spectator flight).
    #[must_use]
    pub fn is_flying(&self) -> bool {
        self.abilities.lock().flying
    }

    /// Sets the player's flying state.
    pub fn set_flying(&self, flying: bool) {
        self.abilities.lock().flying = flying;
    }

    /// Returns the player's flying speed.
    #[must_use]
    pub fn get_flying_speed(&self) -> f32 {
        self.abilities.lock().flying_speed
    }

    /// Sets the player's flying speed.
    pub fn set_flying_speed(&self, speed: f32) {
        self.abilities.lock().flying_speed = speed;
    }

    /// Returns a copy of the player's abilities.
    #[must_use]
    pub fn get_abilities(&self) -> Abilities {
        self.abilities.lock().clone()
    }

    /// Handles the player abilities packet from the client.
    /// This is sent when the player starts or stops flying.
    pub fn handle_player_abilities(&self, packet: SPlayerAbilities) {
        let mut abilities = self.abilities.lock();

        if abilities.may_fly {
            abilities.flying = packet.is_flying();
        } else if packet.is_flying() {
            // Client tried to fly but isn't allowed - resync abilities
            drop(abilities);
            self.send_abilities();
        }
    }
}
