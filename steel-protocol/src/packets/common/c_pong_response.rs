use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::{play, status};

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Status = status::C_PONG_RESPONSE, Play = play::C_PONG_RESPONSE)]
pub struct CPongResponse {
    pub time: i64,
}

impl CPongResponse {
    #[must_use]
    pub fn new(time: i64) -> Self {
        Self { time }
    }
}
