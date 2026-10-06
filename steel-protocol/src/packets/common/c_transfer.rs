use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::{config, play};
use steel_utils::{
    codec::VarInt,
    serial::{PrefixedWrite, WriteTo},
};

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Config = config::C_TRANSFER, Play = play::C_TRANSFER)]
pub struct CTransfer<'a> {
    pub host: &'a str,
    pub port: i32,
}

impl<'a> CTransfer<'a> {
    #[must_use]
    pub const fn new(host: &'a str, port: i32) -> Self {
        Self { host, port }
    }
}

impl WriteTo for CTransfer<'_> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.host.write_prefixed::<VarInt>(writer)?;
        VarInt(self.port).write(writer)
    }
}
