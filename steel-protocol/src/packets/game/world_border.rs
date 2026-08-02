use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::{
    C_INITIALIZE_BORDER, C_SET_BORDER_CENTER, C_SET_BORDER_LERP_SIZE, C_SET_BORDER_SIZE,
    C_SET_BORDER_WARNING_DELAY, C_SET_BORDER_WARNING_DISTANCE,
};

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

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_CENTER)]
pub struct CSetBorderCenter {
    pub new_center_x: f64,
    pub new_center_z: f64,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_LERP_SIZE)]
pub struct CSetBorderLerpSize {
    pub old_size: f64,
    pub new_size: f64,
    #[write(as = VarLong)]
    pub lerp_time: i64,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_SIZE)]
pub struct CSetBorderSize {
    pub size: f64,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_WARNING_DELAY)]
pub struct CSetBorderWarningDelay {
    #[write(as = VarInt)]
    pub warning_delay: i32,
}

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_BORDER_WARNING_DISTANCE)]
pub struct CSetBorderWarningDistance {
    #[write(as = VarInt)]
    pub warning_blocks: i32,
}
