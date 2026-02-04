//! Clientbound set entity motion packet - sent to update entity velocity.

use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::packets::play::C_SET_ENTITY_MOTION;
use steel_utils::{codec::VarInt, serial::WriteTo};

use super::write_lp_vec3;

/// Sent to update an entity's velocity on the client.
///
/// Velocity is sent in LpVec3 format (same as spawn packet).
/// This is used for:
/// - Items landing on ground (velocity zeroed)
/// - Knockback effects
/// - Explosions
/// - Any physics-driven velocity change
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_ENTITY_MOTION)]
pub struct CSetEntityMotion {
    /// The entity ID whose velocity is being updated.
    pub entity_id: i32,
    /// X velocity (blocks/tick).
    pub velocity_x: f64,
    /// Y velocity (blocks/tick).
    pub velocity_y: f64,
    /// Z velocity (blocks/tick).
    pub velocity_z: f64,
}

impl CSetEntityMotion {
    /// Creates a new set entity motion packet.
    #[must_use]
    pub fn new(entity_id: i32, velocity_x: f64, velocity_y: f64, velocity_z: f64) -> Self {
        Self {
            entity_id,
            velocity_x,
            velocity_y,
            velocity_z,
        }
    }
}

impl WriteTo for CSetEntityMotion {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.entity_id).write(writer)?;
        write_lp_vec3(writer, self.velocity_x, self.velocity_y, self.velocity_z)
    }
}
