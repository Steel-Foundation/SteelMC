use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ENTITY_EVENT;
use steel_utils::entity_events::EntityStatus;

/// Performs an entity event.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ENTITY_EVENT)]
pub struct CEntityEvent {
    pub entity_id: i32,
    pub event: EntityStatus,
}
