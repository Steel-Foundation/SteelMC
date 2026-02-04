use std::io::Cursor;

use steel_macros::ServerPacket;
use steel_utils::BlockPos;
use steel_utils::serial::{PrefixedRead, ReadFrom};

/// Maximum characters per sign line.
pub const MAX_SIGN_LINE_LENGTH: usize = 384;

/// Serverbound packet sent when a player finishes editing a sign.
#[derive(ServerPacket, Clone, Debug)]
pub struct SSignUpdate {
    /// The position of the sign block.
    pub pos: BlockPos,
    /// Whether updating the front text (true) or back text (false).
    pub is_front_text: bool,
    /// The four lines of text. Each line is max 384 characters.
    pub lines: [String; 4],
}

impl ReadFrom for SSignUpdate {
    fn read(data: &mut Cursor<&[u8]>) -> std::io::Result<Self> {
        use steel_utils::codec::VarInt;

        let pos = BlockPos::read(data)?;
        let is_front_text = bool::read(data)?;
        let lines = [
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
            String::read_prefixed_bound::<VarInt>(data, MAX_SIGN_LINE_LENGTH)?,
        ];

        Ok(Self {
            pos,
            is_front_text,
            lines,
        })
    }
}
