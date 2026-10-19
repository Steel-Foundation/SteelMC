//! Clientbound entity position sync packet - authoritative entity position update.

use std::io::{self, Write};

use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ENTITY_POSITION_SYNC;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo};

/// One position along a stepped movement path.
#[derive(Clone, Copy, Debug, PartialEq, WriteTo)]
pub struct PositionStep {
    /// Position reached at this step.
    pub position: DVec3,
    /// Ticks elapsed since the previous step.
    #[write(as = VarInt)]
    pub tick_offset: i32,
}

/// The path an entity took to reach its current position.
#[derive(Clone, Debug, PartialEq)]
pub enum PositionPath {
    /// The entity moved straight to a single position.
    Linear(DVec3),
    /// The entity moved through intermediate positions the client replays in order.
    Stepped(Vec<PositionStep>),
}

impl PositionPath {
    /// Returns the position the entity ends up at.
    #[must_use]
    pub fn end_position(&self) -> Option<DVec3> {
        match self {
            Self::Linear(position) => Some(*position),
            Self::Stepped(steps) => steps.last().map(|step| step.position),
        }
    }
}

impl WriteTo for PositionPath {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        match self {
            Self::Linear(position) => {
                VarInt(0).write(writer)?;
                position.write(writer)
            }
            Self::Stepped(steps) => {
                VarInt(1).write(writer)?;
                steps.write_prefixed::<VarInt>(writer)
            }
        }
    }
}

/// Synchronizes an entity's absolute position and rotation.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ENTITY_POSITION_SYNC)]
pub struct CEntityPositionSync {
    /// The entity being synchronized.
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The path the entity took to its current position.
    pub path: PositionPath,
    /// Rotation around the Y axis, in degrees.
    pub y_rot: f32,
    /// Rotation around the X axis, in degrees.
    pub x_rot: f32,
    /// Whether the entity is on the ground.
    pub on_ground: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(packet: &CEntityPositionSync) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.write(&mut bytes).expect("packet should encode");
        bytes
    }

    #[test]
    fn linear_path_writes_type_then_position() {
        let bytes = encode(&CEntityPositionSync {
            entity_id: 12,
            path: PositionPath::Linear(DVec3::new(1.0, 2.0, 3.0)),
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: true,
        });

        let mut expected = vec![12, 0];
        for value in [1.0f64, 2.0, 3.0] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.push(1);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn stepped_path_writes_prefixed_steps() {
        let bytes = encode(&CEntityPositionSync {
            entity_id: 12,
            path: PositionPath::Stepped(vec![PositionStep {
                position: DVec3::new(1.0, 2.0, 3.0),
                tick_offset: 2,
            }]),
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: false,
        });

        let mut expected = vec![12, 1, 1];
        for value in [1.0f64, 2.0, 3.0] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.push(2);
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.push(0);

        assert_eq!(bytes, expected);
    }
}
