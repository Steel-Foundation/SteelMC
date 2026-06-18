//! Clientbound set passengers packet.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_PASSENGERS;

/// Updates the direct passengers riding a vehicle entity.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_PASSENGERS)]
pub struct CSetPassengers {
    /// Vehicle entity id.
    #[write(as = VarInt)]
    pub vehicle_id: i32,
    /// Direct passenger entity ids.
    #[write(as = Prefixed(VarInt, inner = VarInt))]
    pub passenger_ids: Vec<i32>,
}

impl CSetPassengers {
    /// Creates a passenger-list packet for one vehicle.
    #[must_use]
    pub fn new(vehicle_id: i32, passenger_ids: Vec<i32>) -> Self {
        Self {
            vehicle_id,
            passenger_ids,
        }
    }
}
