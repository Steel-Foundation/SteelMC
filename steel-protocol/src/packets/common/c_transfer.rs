use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::{config, play};
use steel_utils::{
    codec::VarInt,
    serial::{PrefixedWrite, WriteTo},
};

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Config = config::C_TRANSFER, Play = play::C_TRANSFER)]
pub struct CTransfer {
    pub host: String,
    pub port: i32,
}

impl CTransfer {
    #[must_use]
    pub fn new(hostname: &str, port: i32) -> Self {
        Self {
            host: hostname.to_string(),
            port,
        }
    }
}

impl WriteTo for CTransfer {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.host.write_prefixed::<VarInt>(writer)?;
        VarInt(self.port).write(writer)
    }
}
