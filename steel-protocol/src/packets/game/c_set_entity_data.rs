//! Clientbound set entity data packet - sent to sync entity metadata.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::{
    entity_data::{DataValue, write_data_values},
    packets::play::C_SET_ENTITY_DATA,
};
use steel_utils::{codec::VarInt, serial::WriteTo};

/// Sent to synchronize entity metadata (health, pose, flags, etc.) with the client.
///
/// The packet contains a list of changed metadata values, each with:
/// - `index`: The field index (0-254)
/// - `serializer_id`: The type ID from EntityDataSerializers
/// - `value`: The actual data
///
/// The list is terminated by a 0xFF byte.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_ENTITY_DATA)]
pub struct CSetEntityData {
    /// The entity ID whose metadata is being updated.
    pub entity_id: i32,
    /// The metadata values to sync.
    pub packed_items: Vec<DataValue>,
}

impl CSetEntityData {
    /// Creates a new set entity data packet.
    #[must_use]
    pub fn new(entity_id: i32, packed_items: Vec<DataValue>) -> Self {
        Self {
            entity_id,
            packed_items,
        }
    }
}

impl WriteTo for CSetEntityData {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.entity_id).write(writer)?;
        let mut buf = Vec::new();
        write_data_values(&self.packed_items, &mut buf)?;
        writer.write_all(&buf)
    }
}
