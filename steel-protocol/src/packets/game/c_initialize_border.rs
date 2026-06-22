use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_INITIALIZE_BORDER;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_INITIALIZE_BORDER)]
pub struct CInitializeBorder {
    pub new_center_x: f64,
    pub new_center_z: f64,
    pub old_size: f64,
    pub new_size: f64,
    #[write(as = VarLong)]
    pub lerp_time: i64,
    #[write(as = VarInt)]
    pub new_absolute_max_size: i32,
    #[write(as = VarInt)]
    pub warning_blocks: i32,
    #[write(as = VarInt)]
    pub warning_time: i32,
}
