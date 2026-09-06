//! Serverbound packet for selecting a beacon's primary and secondary effects.

use std::io::Cursor;

use steel_macros::ServerPacket;
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom;

/// Sent when the player confirms a beacon's effect selection.
///
/// Vanilla 26.2 encodes each effect as `ByteBufCodecs.optional(MobEffect.STREAM_CODEC)`:
/// a presence boolean, then the mob effect's raw registry id as a `VarInt`.
#[derive(ServerPacket, Clone, Debug)]
pub struct SSetBeacon {
    pub primary: Option<i32>,
    pub secondary: Option<i32>,
}

impl ReadFrom for SSetBeacon {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        Ok(Self {
            primary: read_optional_effect(data)?,
            secondary: read_optional_effect(data)?,
        })
    }
}

fn read_optional_effect(data: &mut Cursor<&[u8]>) -> std::io::Result<Option<i32>> {
    if !bool::read(data)? {
        return Ok(None);
    }
    Ok(Some(VarInt::read(data)?.0))
}
