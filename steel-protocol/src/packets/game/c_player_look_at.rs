use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_PLAYER_LOOK_AT;

/// Entity position used as one endpoint of a player look-at operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, WriteTo)]
#[write(as = VarInt)]
pub enum LookAtAnchor {
    /// The entity's base position.
    Feet = 0,
    /// The entity's eye position.
    Eyes = 1,
}

#[derive(Clone, Debug, WriteTo)]
struct LookAtEntity {
    #[write(as = VarInt)]
    entity_id: i32,
    anchor: LookAtAnchor,
}

/// Rotates the receiving player toward a position or tracked entity.
#[derive(ClientPacket, Clone, Debug, WriteTo)]
#[packet_id(Play = C_PLAYER_LOOK_AT)]
pub struct CPlayerLookAt {
    from_anchor: LookAtAnchor,
    fallback_position: DVec3,
    target: Option<LookAtEntity>,
}

impl CPlayerLookAt {
    #[must_use]
    pub const fn position(from_anchor: LookAtAnchor, position: DVec3) -> Self {
        Self {
            from_anchor,
            fallback_position: position,
            target: None,
        }
    }

    #[must_use]
    pub const fn entity(
        from_anchor: LookAtAnchor,
        fallback_position: DVec3,
        entity_id: i32,
        target_anchor: LookAtAnchor,
    ) -> Self {
        Self {
            from_anchor,
            fallback_position,
            target: Some(LookAtEntity {
                entity_id,
                anchor: target_anchor,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_utils::serial::WriteTo as _;

    use super::{CPlayerLookAt, LookAtAnchor};

    #[test]
    fn position_target_encodes_without_entity_suffix() {
        let packet = CPlayerLookAt::position(LookAtAnchor::Eyes, DVec3::new(1.0, 2.0, 3.0));
        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

        assert_eq!(encoded.len(), 26);
        assert_eq!(encoded[0], 1);
        assert_eq!(encoded[25], 0);
    }

    #[test]
    fn entity_target_encodes_id_and_anchor_after_presence_flag() {
        let packet = CPlayerLookAt::entity(
            LookAtAnchor::Feet,
            DVec3::new(1.0, 2.0, 3.0),
            42,
            LookAtAnchor::Eyes,
        );
        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

        assert_eq!(encoded.len(), 28);
        assert_eq!(encoded[0], 0);
        assert_eq!(&encoded[25..], &[1, 42, 1]);
    }
}
