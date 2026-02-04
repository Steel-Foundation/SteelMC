//! Clientbound take item entity packet - sent when an entity picks up an item.
//!
//! Triggers the pickup animation and sound on the client. The item entity
//! will fly towards the collector before being removed.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_TAKE_ITEM_ENTITY;
use steel_utils::{codec::VarInt, serial::WriteTo};

/// Sent when an entity picks up an item (item, experience orb, or arrow).
///
/// Triggers the pickup animation on the client where the item entity
/// flies towards the collector entity before disappearing.
///
/// Corresponds to vanilla's `ClientboundTakeItemEntityPacket`.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_TAKE_ITEM_ENTITY)]
pub struct CTakeItemEntity {
    /// The entity ID of the item being picked up.
    pub item_id: i32,
    /// The entity ID of the collector (player or other entity).
    pub player_id: i32,
    /// The number of items picked up (for animation/sound).
    pub amount: i32,
}

impl CTakeItemEntity {
    /// Creates a new take item entity packet.
    #[must_use]
    pub fn new(item_id: i32, player_id: i32, amount: i32) -> Self {
        Self {
            item_id,
            player_id,
            amount,
        }
    }
}

impl WriteTo for CTakeItemEntity {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.item_id).write(writer)?;
        VarInt(self.player_id).write(writer)?;
        VarInt(self.amount).write(writer)
    }
}
