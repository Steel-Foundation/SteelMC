use std::io::{Cursor, Read};

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::Identifier;

use steel_utils::serial::ReadFrom;

#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SCustomPayload {
    pub identifier: Identifier,
    //#[read(as = "vec")]
    pub payload: Payload,
}

#[derive(Clone, Debug)]
pub struct Payload(pub Vec<u8>);

impl ReadFrom for Payload {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self, std::io::Error> {
        let mut buf = vec![];
        data.read_to_end(&mut buf)?;
        Ok(Self(buf))
    }
}
