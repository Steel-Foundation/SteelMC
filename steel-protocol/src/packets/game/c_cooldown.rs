//! Clientbound item cooldown packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_COOLDOWN;
use steel_utils::Identifier;

/// Starts or clears a client-side item cooldown group.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_COOLDOWN)]
pub struct CCooldown {
    pub cooldown_group: Identifier,
    #[write(as = VarInt)]
    pub duration: i32,
}
