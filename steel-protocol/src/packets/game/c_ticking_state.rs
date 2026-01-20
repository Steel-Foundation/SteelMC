use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_TICKING_STATE;

/// Packet sent to clients to inform them of the current tick rate and frozen state.
/// This allows clients to adjust their local tick rate manager for smoother gameplay.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_TICKING_STATE)]
pub struct CTickingState {
    /// The current tick rate (ticks per second).
    pub tick_rate: f32,
    /// Whether the server is currently frozen.
    pub is_frozen: bool,
}

impl CTickingState {
    /// Creates a new ticking state packet.
    #[must_use]
    pub fn new(tick_rate: f32, is_frozen: bool) -> Self {
        Self {
            tick_rate,
            is_frozen,
        }
    }
}
