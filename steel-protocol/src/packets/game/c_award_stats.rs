use std::io::Write;
use steel_macros::ClientPacket;
use steel_registry::packets::play::C_AWARD_STATS;
use steel_registry::stat::Stat;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;

/// Clientbound packet that updates the client with the stats that have been
/// marked dirty.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_AWARD_STATS)]
pub struct CAwardStats {
    /// The dirty stats to send.
    pub stats: Vec<(Stat, i32)>,
}

impl WriteTo for CAwardStats {
    fn write(&self, writer: &mut impl Write) -> std::io::Result<()> {
        let visible = || {
            self.stats
                .iter()
                .filter(|(stat, _)| stat.stat_type().is_client_value(stat.stat_value_id()))
        };
        VarInt(visible().count() as i32).write(writer)?;
        for (stat, count) in visible() {
            stat.write(writer)?;
            VarInt(*count).write(writer)?;
        }
        Ok(())
    }
}
