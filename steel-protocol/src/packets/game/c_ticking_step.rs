use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_TICKING_STEP;

/// Packet sent to clients to inform them of the number of frozen ticks to run.
/// This is used when stepping forward while the server is frozen.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_TICKING_STEP)]
pub struct CTickingStep {
    /// The number of ticks to step forward.
    #[write(as = VarInt)]
    pub tick_steps: i32,
}

impl CTickingStep {
    /// Creates a new ticking step packet.
    #[must_use]
    pub fn new(tick_steps: i32) -> Self {
        Self { tick_steps }
    }
}
