use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PROJECTILE_POWER;

/// Syncs a hurting projectile's acceleration power to tracking clients.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_PROJECTILE_POWER)]
pub struct CProjectilePower {
    #[write(as = VarInt)]
    pub entity_id: i32,
    pub acceleration_power: f64,
}
