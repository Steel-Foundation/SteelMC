use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::login::C_LOGIN_COMPRESSION;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Login = C_LOGIN_COMPRESSION)]
pub struct CLoginCompression {
    #[write(as = VarInt)]
    pub threshold: i32,
}

impl CLoginCompression {
    #[must_use]
    pub const fn new(threshold: i32) -> Self {
        Self { threshold }
    }
}
