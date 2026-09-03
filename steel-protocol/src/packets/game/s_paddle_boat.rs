use std::io::{Cursor, Result};

use steel_macros::ServerPacket;
use steel_utils::serial::ReadFrom;

/// Left/right paddle state sent by the client while controlling a boat.
///
/// Mirrors vanilla `ServerboundPaddleBoatPacket`.
#[derive(ServerPacket, Clone, Debug)]
pub struct SPaddleBoat {
    pub left_paddle: bool,
    pub right_paddle: bool,
}

impl ReadFrom for SPaddleBoat {
    fn read(data: &mut Cursor<&[u8]>) -> Result<Self> {
        Ok(Self {
            left_paddle: bool::read(data)?,
            right_paddle: bool::read(data)?,
        })
    }
}
