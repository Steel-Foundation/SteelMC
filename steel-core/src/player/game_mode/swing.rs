use super::{InteractionHand, InteractionResult, LivingEntity, Player, SwingAnimation};

impl Player {
    /// sends the animation to tracking players and returns whether a swing started
    pub fn swing(
        &self,
        hand: InteractionHand,
        animation: SwingAnimation,
        update_self: bool,
    ) -> bool {
        LivingEntity::swing(self, hand, animation, update_self)
    }

    /// resets attack strength only when the requested swing starts
    pub fn swing_and_reset_attack_strength(
        &self,
        hand: InteractionHand,
        animation: SwingAnimation,
        update_self: bool,
    ) {
        if self.swing(hand, animation, update_self) {
            self.reset_attack_strength_ticker();
        }
    }

    pub(in crate::player) fn swing_after_interaction(
        &self,
        hand: InteractionHand,
        animation: SwingAnimation,
        result: InteractionResult,
    ) {
        if result.should_swing() {
            self.swing_and_reset_attack_strength(
                hand,
                animation,
                result == InteractionResult::SuccessServer,
            );
        }
    }
}
