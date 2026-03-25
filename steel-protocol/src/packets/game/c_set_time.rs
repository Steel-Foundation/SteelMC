use std::io::{Result, Write};
use steel_macros::ClientPacket;
use steel_registry::RegistryExt;
use steel_registry::packets::play::C_SET_TIME;
use steel_utils::codec::{VarInt, VarLong};
use steel_utils::serial::WriteTo;

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_TIME)]
pub struct CSetTime {
    pub game_time: i64,
    /// (clock_registry_id, total_ticks, partial_tick, rate)
    pub clock_updates: Vec<(i32, i64, f32, f32)>,
}

impl WriteTo for CSetTime {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.game_time.write(writer)?;
        VarInt(self.clock_updates.len() as i32).write(writer)?;
        for &(clock_id, total_ticks, partial_tick, rate) in &self.clock_updates {
            VarInt(clock_id).write(writer)?;
            VarLong(total_ticks).write(writer)?;
            partial_tick.write(writer)?;
            rate.write(writer)?;
        }
        Ok(())
    }
}

impl CSetTime {
    #[must_use]
    pub fn new(game_time: i64, day_time: i64, partial_tick: f32, rate: f32) -> Self {
        use steel_registry::{REGISTRY, vanilla_world_clocks};
        let clock_id = REGISTRY
            .world_clocks
            .id_from_key(&vanilla_world_clocks::OVERWORLD.key)
            .unwrap() as i32;
        Self {
            game_time,
            clock_updates: vec![(clock_id, day_time, partial_tick, rate)],
        }
    }
}
