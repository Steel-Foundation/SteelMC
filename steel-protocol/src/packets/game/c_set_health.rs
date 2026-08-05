use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_HEALTH;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_HEALTH)]
pub struct CSetHealth {
    /// 0 or less = dead, 20 = full HP.
    pub health: f32,
    /// 0–20
    #[write(as = VarInt)]
    pub food: i32,
    /// Seems to vary from 0.0 to 5.0 in integer increments.
    pub food_saturation: f32,
}
