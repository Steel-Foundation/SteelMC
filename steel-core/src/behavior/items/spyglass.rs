//! Spyglass item behavior.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events;

use crate::behavior::{InteractionResult, ItemBehavior, ItemUseAnimation, UseItemContext};
use crate::entity::{Entity, LivingEntity};
use crate::world::World;

const USE_DURATION: i32 = 1200;

/// Vanilla spyglass active-use behavior.
#[item_behavior]
pub struct SpyglassItem;

impl SpyglassItem {
    fn stop_using(user: &dyn LivingEntity) {
        user.play_sound(&sound_events::ITEM_SPYGLASS_STOP_USING, 1.0, 1.0);
    }
}

impl ItemBehavior for SpyglassItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        context
            .player
            .play_sound(&sound_events::ITEM_SPYGLASS_USE, 1.0, 1.0);
        // TODO: Award `Stats.ITEM_USED` once Steel has a statistics foundation.
        // player.awardStat(Stats.ITEM_USED.get(this));
        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Spyglass
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        USE_DURATION
    }

    fn finish_using(
        &self,
        stack: &mut ItemStack,
        _world: &Arc<World>,
        user: &dyn LivingEntity,
    ) -> ItemStack {
        Self::stop_using(user);
        stack.copy_with_count(stack.count())
    }

    fn release_using(
        &self,
        _stack: &mut ItemStack,
        _world: &Arc<World>,
        user: &dyn LivingEntity,
        _time_left: i32,
    ) -> bool {
        Self::stop_using(user);
        true
    }
}
