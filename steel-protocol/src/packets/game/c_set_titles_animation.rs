use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_TITLES_ANIMATION;

/// Sets the title fade-in, stay, and fade-out durations on the client.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_TITLES_ANIMATION)]
pub struct CSetTitlesAnimation {
    /// Number of client ticks used for the fade-in.
    pub fade_in: i32,
    /// Number of client ticks the title remains fully visible.
    pub stay: i32,
    /// Number of client ticks used for the fade-out.
    pub fade_out: i32,
}

impl CSetTitlesAnimation {
    /// Creates a title-animation packet.
    #[must_use]
    pub const fn new(fade_in: i32, stay: i32, fade_out: i32) -> Self {
        Self {
            fade_in,
            stay,
            fade_out,
        }
    }
}
