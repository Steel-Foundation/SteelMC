use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_OPEN_SIGN_EDITOR;
use steel_utils::BlockPos;

/// Clientbound packet sent to open the sign editor GUI.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_OPEN_SIGN_EDITOR)]
pub struct COpenSignEditor {
    /// The position of the sign block.
    pub pos: BlockPos,
    /// Whether to edit the front text (true) or back text (false).
    pub is_front_text: bool,
}
