//! Packet to respawn a player or switch dimensions.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_RESPAWN;
use steel_utils::Identifier;
use steel_utils::serial::write::{OptionalBlockPos, OptionalIdentifier};

/// Respawn a player in any dimension.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_RESPAWN)]
pub struct CRespawn {
    /// The dimension type registry ID.
    #[write(as = VarInt)]
    pub dimension_type: i32,
    /// The dimension name (e.g. `minecraft:overworld`).
    pub dimension_name: Identifier,
    /// Hashed seed for biome noise.
    pub hashed_seed: i64,
    /// Current game mode.
    pub gamemode: u8,
    /// Previous game mode (-1 if none).
    pub previous_gamemode: i8,
    /// Whether this is a debug world.
    pub is_debug: bool,
    /// Whether this is a superflat world.
    pub is_flat: bool,
    /// Whether the player has a death location.
    pub has_death_location: bool,
    /// The dimension of the death location, if any.
    pub death_dimension_name: OptionalIdentifier,
    /// The death location, if any.
    pub death_location: OptionalBlockPos,
    /// Portal cooldown in ticks.
    #[write(as = VarInt)]
    pub portal_cooldown_ticks: i32,
    /// Sea level for the dimension.
    #[write(as = VarInt)]
    pub sea_level: i32,
    /// Bit flags for which data to keep across respawn.
    pub data_kept: i8,
}

impl CRespawn {
    /// Keep attribute modifiers across respawn.
    pub const KEEP_ATTRIBUTES: i8 = 1;
    /// Keep entity data across respawn.
    pub const KEEP_ENTITY_DATA: i8 = 2;
    /// Keep all data across respawn.
    pub const KEEP_ALL: i8 = 3;
}
