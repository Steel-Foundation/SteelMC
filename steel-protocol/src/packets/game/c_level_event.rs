use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LEVEL_EVENT;
use steel_utils::BlockPos;

/// Sent to trigger level events (sounds, particles, animations) on the client.
///
/// Level events are predefined effects identified by an event type constant.
/// The `data` field provides event-specific parameters (e.g., block state ID for
/// block destruction particles).
///
/// See `steel_registry::level_events` for all available event type constants.
#[derive(WriteTo, ClientPacket, Clone, Debug)]
#[packet_id(Play = C_LEVEL_EVENT)]
pub struct CLevelEvent {
    /// The event type ID. Use constants from `steel_registry::level_events`.
    pub event_type: i32,
    /// The position where the event occurs.
    pub pos: BlockPos,
    /// Event-specific data (e.g., block state ID for `PARTICLES_DESTROY_BLOCK`).
    pub data: i32,
    /// If true, the event is sent to all players regardless of distance.
    /// If false, only players within 64 blocks receive it.
    pub global_event: bool,
}

impl CLevelEvent {
    /// Creates a new level event packet.
    #[must_use]
    pub fn new(event_type: i32, pos: BlockPos, data: i32, global_event: bool) -> Self {
        Self {
            event_type,
            pos,
            data,
            global_event,
        }
    }

    /// Creates a block destruction event with particles and sound.
    ///
    /// # Arguments
    /// * `pos` - The position of the destroyed block
    /// * `block_state_id` - The block state ID of the destroyed block
    #[must_use]
    pub fn destroy_block(pos: BlockPos, block_state_id: u32) -> Self {
        Self::new(
            steel_registry::level_events::PARTICLES_DESTROY_BLOCK,
            pos,
            block_state_id as i32,
            false,
        )
    }
}
