use std::io::{Cursor, Error, Read, Result};

use steel_macros::ServerPacket;
use steel_utils::{codec::VarInt, serial::ReadFrom};

/// Vanilla `ServerboundCustomQueryAnswerPacket.MAX_PAYLOAD_SIZE`, in bytes.
const MAX_PAYLOAD_SIZE: usize = 1_048_576;

/// Login plugin response. `payload` is `None` when the client did not understand the query.
#[derive(ServerPacket, Clone, Debug)]
pub struct SCustomQueryAnswer {
    pub transaction_id: i32,
    pub payload: Option<Vec<u8>>,
}

impl ReadFrom for SCustomQueryAnswer {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        let transaction_id = VarInt::read(data)?.0;
        let payload = if bool::read(data)? {
            let mut payload = Vec::new();
            data.take(MAX_PAYLOAD_SIZE as u64 + 1)
                .read_to_end(&mut payload)?;
            if payload.len() > MAX_PAYLOAD_SIZE {
                return Err(Error::other("Payload may not be larger than 1048576 bytes"));
            }
            Some(payload)
        } else {
            None
        };
        Ok(Self {
            transaction_id,
            payload,
        })
    }
}
