//! Packet to remove players from the player list (tab menu).

use steel_registry::packets::play::C_RESPAWN;
use steel_macros::{ClientPacket, WriteTo};
use steel_utils::{Identifier};
use steel_utils::serial::write::{OptionalBlockPos, OptionalIdentifier};

/// respawn a player in any dimension
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_RESPAWN)]
pub struct CRespawn {
    #[write(as = VarInt)]
    dimension_type: i32,
    dimension_name: Identifier,
    hashed_seed: i64,
    gamemode: u8,
    previous_gamemode: i8,
    is_debug: bool,
    is_flat: bool,
    has_death_location: bool,
    death_dimension_name: OptionalIdentifier,
    death_location: OptionalBlockPos,
    #[write(as = VarInt)]
    portal_cooldown_ticks: i32,
    #[write(as = VarInt)]
    sea_level: i32,
    data_kept: i8
}
