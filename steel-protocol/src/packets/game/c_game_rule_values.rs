use std::io::Write;

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_GAME_RULE_VALUES;
use steel_utils::{
    Identifier,
    codec::VarInt,
    serial::{PrefixedWrite, WriteTo},
};

/// Sends the current game rule values to the client after a gamemaster requests them.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_GAME_RULE_VALUES)]
pub struct CGameRuleValues {
    /// Game rule keys paired with their serialized values.
    pub values: Vec<(Identifier, String)>,
}

impl WriteTo for CGameRuleValues {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        VarInt(self.values.len() as i32).write(writer)?;
        for (key, value) in &self.values {
            key.write(writer)?;
            value.write_prefixed::<VarInt>(writer)?;
        }
        Ok(())
    }
}
