use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::login::C_CUSTOM_QUERY;
use steel_utils::{Identifier, codec::VarInt, serial::WriteTo};

/// Login plugin request. The payload is written raw, without a length prefix.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Login = C_CUSTOM_QUERY)]
pub struct CCustomQuery<'a> {
    pub transaction_id: i32,
    pub identifier: Identifier,
    pub payload: &'a [u8],
}

impl<'a> CCustomQuery<'a> {
    #[must_use]
    pub const fn new(transaction_id: i32, identifier: Identifier, payload: &'a [u8]) -> Self {
        Self {
            transaction_id,
            identifier,
            payload,
        }
    }
}

impl WriteTo for CCustomQuery<'_> {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.transaction_id).write(writer)?;
        self.identifier.write(writer)?;
        writer.write_all(self.payload)
    }
}
