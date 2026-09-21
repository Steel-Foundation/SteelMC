use steel_macros::{ClientPacket, WriteTo};
use steel_registry::advancement::AdvancementProgressData;
use steel_registry::advancement::registry::AdvancementRef;
use steel_registry::packets::play::C_UPDATE_ADVANCEMENTS;
use steel_utils::Identifier;

/// Packet sent to clients to inform them of the number of frozen ticks to run.
/// This is used when stepping forward while the server is frozen.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_UPDATE_ADVANCEMENTS)]
pub struct CUpdateAdvancements {
    /// The number of ticks to step forward.
    pub reset: bool,
    pub added: Vec<AdvancementRef>,
    pub removed: Vec<Identifier>,
    pub progress: Vec<AdvancementProgressData>,
    pub show_advancements: bool,
}

impl CUpdateAdvancements {
    /// Creates a new update advancement packet.
    #[must_use]
    pub const fn new(
        reset: bool,
        added: Vec<AdvancementRef>,
        progress: Vec<AdvancementProgressData>,
        removed: Vec<Identifier>,
        show_advancements: bool,
    ) -> Self {
        Self {
            reset,
            added,
            removed,
            progress,
            show_advancements,
        }
    }
}
