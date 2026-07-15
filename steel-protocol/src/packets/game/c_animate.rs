//! Clientbound animate packet - sent to play an entity animation.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ANIMATE;

/// Animation action types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, WriteTo)]
#[repr(u8)]
#[write(as = u8)]
pub enum AnimateAction {
    /// Swing main hand
    SwingMainHand = 0,
    /// Wake up from bed
    WakeUp = 2,
    /// Swing off hand
    SwingOffHand = 3,
    /// Critical hit effect
    CriticalHit = 4,
    /// Magic critical hit effect (enchanted weapon)
    MagicCriticalHit = 5,
}

/// Sent to play an animation on an entity.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ANIMATE)]
pub struct CAnimate {
    /// The entity ID to animate.
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The animation action to play.
    pub action: AnimateAction,
}

impl CAnimate {
    /// Creates a new animate packet.
    #[must_use]
    pub const fn new(entity_id: i32, action: AnimateAction) -> Self {
        Self { entity_id, action }
    }

    /// Creates a swing main hand animation.
    #[must_use]
    pub const fn swing_main_hand(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::SwingMainHand)
    }

    /// Creates a swing off hand animation.
    #[must_use]
    pub const fn swing_off_hand(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::SwingOffHand)
    }

    /// Creates a critical hit animation.
    #[must_use]
    pub const fn critical_hit(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::CriticalHit)
    }

    /// Creates a magic critical hit animation.
    #[must_use]
    pub const fn magic_critical_hit(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::MagicCriticalHit)
    }
}
