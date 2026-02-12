use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_TIME;

#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_TIME)]
pub struct CSetTime {
    pub game_time: i64,
    pub day_time: i64,
    pub time_of_day_increasing: bool,
}

impl CSetTime {
    #[must_use]
    pub fn new(game_time: i64, day_time: i64, time_of_day_increasing: bool) -> Self {
        Self {
            game_time,
            day_time,
            time_of_day_increasing,
        }
    }
}
