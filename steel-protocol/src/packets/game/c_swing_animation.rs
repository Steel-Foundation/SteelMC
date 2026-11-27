//! Clientbound swing animation packet - sent when an entity swings a hand.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::data_components::vanilla_components::SwingAnimation;
use steel_registry::packets::play::C_SWING_ANIMATION;
use steel_utils::types::InteractionHand;

/// Sent to play an arm swing on an entity.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SWING_ANIMATION)]
pub struct CSwingAnimation {
    /// The entity ID that is swinging.
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The hand being swung.
    pub hand: InteractionHand,
    /// The animation style and its duration in ticks.
    pub animation: SwingAnimation,
}

impl CSwingAnimation {
    /// Creates a new swing animation packet.
    #[must_use]
    pub const fn new(entity_id: i32, hand: InteractionHand, animation: SwingAnimation) -> Self {
        Self {
            entity_id,
            hand,
            animation,
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::data_components::components::SwingAnimationType;
    use steel_utils::codec::VarInt;
    use steel_utils::serial::WriteTo as _;

    use super::{CSwingAnimation, InteractionHand, SwingAnimation};

    #[test]
    fn writes_entity_hand_and_animation() {
        let mut bytes = Vec::new();
        CSwingAnimation::new(
            7,
            InteractionHand::OffHand,
            SwingAnimation::new(SwingAnimationType::Stab, 13),
        )
        .write(&mut bytes)
        .expect("write should succeed");

        let mut expected = Vec::new();
        VarInt(7).write(&mut expected).unwrap();
        VarInt(1).write(&mut expected).unwrap();
        VarInt(2).write(&mut expected).unwrap();
        VarInt(13).write(&mut expected).unwrap();
        assert_eq!(bytes, expected);
    }
}
