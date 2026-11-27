use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CLEAR_TITLES;

/// Clears the title display state on the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CLEAR_TITLES)]
pub struct CClearTitles {
    /// Whether the client also resets the title animation times to defaults.
    pub reset_times: bool,
}

impl CClearTitles {
    /// Creates a packet that clears the client's title display state.
    #[must_use]
    pub const fn new(reset_times: bool) -> Self {
        Self { reset_times }
    }
}
