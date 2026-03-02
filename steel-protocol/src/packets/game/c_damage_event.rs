//! Clientbound damage event packet - tells the client an entity took damage.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_DAMAGE_EVENT;
use steel_utils::math::Vector3;

/// Sent when an entity takes damage. Used for hit animations and damage direction indicator.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_DAMAGE_EVENT)]
pub struct CDamageEvent {
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The damage type ID in the `minecraft:damage_type` registry.
    #[write(as = VarInt)]
    pub source_type_id: i32,
    /// The entity ID + 1 of the entity responsible for the damage, or 0 if none.
    #[write(as = VarInt)]
    pub source_cause_id: i32,
    /// The entity ID + 1 of the entity that directly dealt the damage, or 0 if none.
    #[write(as = VarInt)]
    pub source_direct_id: i32,
    /// Optional source position (e.g. for explosions).
    /// Encoded as bool-prefixed via the `Option<T: WriteTo>` impl.
    pub source_position: Option<Vector3<f64>>,
}
