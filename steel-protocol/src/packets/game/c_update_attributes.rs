//! Clientbound update attributes packet - sent to sync entity attributes with modifiers.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_UPDATE_ATTRIBUTES;
use steel_utils::Identifier;

/// Represents a single attribute modifier within an attribute snapshot.
#[derive(WriteTo, Clone, Debug)]
pub struct AttributeModifierData {
    /// The resource location identifier for this modifier (e.g. `minecraft:sprinting`).
    pub id: Identifier,
    /// The modifier amount.
    pub amount: f64,
    /// The operation type for this modifier.
    pub operation: AttributeModifierOperation,
}

/// The operation type for an attribute modifier.
///
/// Matches vanilla `AttributeModifier.Operation`:
/// - `AddValue` (0): `total += amount`
/// - `AddMultipliedBase` (1): `total += base * amount`
/// - `AddMultipliedTotal` (2): `total *= 1 + amount`
#[derive(WriteTo, Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[write(as = VarInt)]
#[expect(
    clippy::enum_variant_names,
    reason = "Since it matches vanilla `AttributeModifier.Operation` and explains what it does i guess"
)]
pub enum AttributeModifierOperation {
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

/// A snapshot of a single attribute's state, including its base value and active modifiers.
#[derive(WriteTo, Clone, Debug)]
pub struct AttributeSnapshot {
    /// The registry ID of the attribute (VarInt on the wire).
    #[write(as = VarInt)]
    pub attribute_id: i32,
    /// The base value of the attribute.
    pub base_value: f64,
    /// Active modifiers on this attribute.
    #[write(as = Prefixed(VarInt))]
    pub modifiers: Vec<AttributeModifierData>,
}

/// Clientbound packet sent to update entity attributes and their modifiers.
///
/// Used for things like sprint speed modifiers, potion effects on speed/health, etc.
/// Vanilla: `ClientboundUpdateAttributesPacket`
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_UPDATE_ATTRIBUTES)]
pub struct CUpdateAttributes {
    /// The entity ID whose attributes are being updated.
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The attribute snapshots to sync.
    #[write(as = Prefixed(VarInt))]
    pub attributes: Vec<AttributeSnapshot>,
}

impl CUpdateAttributes {
    /// Creates a new update attributes packet.
    #[must_use]
    pub fn new(entity_id: i32, attributes: Vec<AttributeSnapshot>) -> Self {
        Self {
            entity_id,
            attributes,
        }
    }
}
