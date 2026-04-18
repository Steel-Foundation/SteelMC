use steel_macros::{ReadFrom, ServerPacket};

/// Action types for the player command packet.
///
/// Maps to vanilla `ServerboundPlayerCommandPacket.Action` enum.
/// Wire format: VarInt (0–6).
#[derive(ReadFrom, Clone, Copy, Debug, PartialEq, Eq)]
#[read(as = VarInt)]
pub enum PlayerCommandAction {
    /// Player leaves bed (only when clicking "Leave Bed" button).
    LeaveBed = 0,
    /// Player starts sprinting.
    StartSprinting = 1,
    /// Player stops sprinting.
    StopSprinting = 2,
    /// Player starts jumping while riding a horse (data = jump boost 0-100).
    StartRidingJump = 3,
    /// Player stops jumping while riding a horse.
    StopRidingJump = 4,
    /// Player opens vehicle inventory (horse/chest boat) via inventory key.
    OpenVehicleInventory = 5,
    /// Player starts flying with elytra.
    StartFallFlying = 6,
}

/// Serverbound packet sent when a player performs a command action.
///
/// This handles actions like sprinting, sleeping, elytra, and horse riding.
/// Packet ID 0x29 (play phase) — `player_command` in packets.json.
#[derive(ReadFrom, ServerPacket, Clone, Debug)]
pub struct SPlayerCommand {
    /// The entity ID of the player (should match the player's ID).
    #[read(as = VarInt)]
    pub entity_id: i32,
    /// The action being performed.
    pub action: PlayerCommandAction,
    /// Jump boost for StartRidingJump (0-100), otherwise 0.
    #[read(as = VarInt)]
    pub data: i32,
}
