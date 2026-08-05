//! Clientbound respawn packet - sent to respawn a player or change dimensions.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_RESPAWN;
use steel_utils::{BlockPos, Identifier};

/// Respawn a player in any dimension.
///
/// Sent by the server when a player respawns after death or changes dimensions.
/// The client will reset its world state and prepare for new chunk data.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_RESPAWN)]
pub struct CRespawn {
    /// The dimension type registry ID.
    #[write(as = VarInt)]
    pub dimension_type: i32,
    /// The dimension name (e.g. `minecraft:overworld`).
    pub dimension_name: Identifier,
    /// The hashed seed of the world (used for biome noise).
    pub hashed_seed: i64,
    /// The player's game mode after respawning.
    pub gamemode: u8,
    /// The player's previous game mode (-1 if none).
    pub previous_gamemode: i8,
    /// Whether this is a debug world.
    pub is_debug: bool,
    /// Whether this is a superflat world.
    pub is_flat: bool,
    /// Whether the player has a death location.
    pub has_death_location: bool,
    /// The dimension name of the death location (if `has_death_location` is true).
    #[write(as = Unprefixed)]
    pub death_dimension_name: Option<Identifier>,
    /// The block position of the death location (if `has_death_location` is true).
    #[write(as = Unprefixed)]
    pub death_location: Option<BlockPos>,
    /// The portal cooldown in ticks.
    #[write(as = VarInt)]
    pub portal_cooldown_ticks: i32,
    /// The sea level of the dimension.
    #[write(as = VarInt)]
    pub sea_level: i32,
    /// Bit field: 0x01 = keep attributes, 0x02 = keep metadata.
    /// Set to 0 for a full reset (normal respawn).
    pub data_kept: i8,
}
