use steel_macros::ClientPacket;
use steel_registry::packets::play::C_LEVEL_EVENT;
use steel_utils::BlockPos;
use steel_utils::serial::WriteTo;

/// Sent to trigger level events (sounds, particles, animations) on the client.
///
/// Level events are predefined effects identified by an event type constant.
/// The `data` field provides event-specific parameters (e.g., block state ID for
/// block destruction particles).
///
/// See `steel_registry::level_events` for all available event type constants.
#[derive(ClientPacket, Clone, Debug)]
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

impl WriteTo for CLevelEvent {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        self.event_type.write(writer)?;
        self.pos.write(writer)?;
        let data = if matches!(
            self.event_type,
            steel_registry::level_events::PARTICLES_DESTROY_BLOCK
                | steel_registry::level_events::PARTICLES_AND_SOUND_DESTROY_BLOCK
                | steel_registry::level_events::PARTICLES_AND_SOUND_BRUSH_BLOCK_COMPLETE
        ) {
            let state = u16::try_from(self.data)
                .map_err(|_| std::io::Error::other("block destruction state is out of range"))?;
            i32::from(
                steel_registry::client_content::client_block_state(steel_utils::BlockStateId(
                    state,
                ))
                .0,
            )
        } else {
            self.data
        };
        data.write(writer)?;
        self.global_event.write(writer)
    }
}

impl CLevelEvent {
    /// Creates a new level event packet.
    #[must_use]
    pub const fn new(event_type: i32, pos: BlockPos, data: i32, global_event: bool) -> Self {
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
    pub const fn destroy_block(pos: BlockPos, block_state_id: u32) -> Self {
        Self::new(
            steel_registry::level_events::PARTICLES_DESTROY_BLOCK,
            pos,
            block_state_id as i32,
            false,
        )
    }
}
