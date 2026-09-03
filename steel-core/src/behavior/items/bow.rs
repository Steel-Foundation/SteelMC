//! Bow item behavior (`BowItem`).
//!
//! Right-click starts a 72000-tick draw (bow animation). Releasing shoots an
//! arrow whose speed scales with draw time. One arrow is consumed unless the
//! shooter is in creative or the bow has Infinity.

use std::sync::Arc;

use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::stat::vanilla_stat_types;
use steel_registry::vanilla_enchantments;
use steel_registry::vanilla_item_tags::ItemTag;
use steel_registry::{REGISTRY, TaggedRegistryExt as _, sound_events};

use crate::behavior::{InteractionResult, ItemBehavior, ItemUseAnimation, UseItemContext};
use crate::entity::entities::ArrowEntity;
use crate::entity::{LivingEntity, spawn_arrow_projectile};
use crate::inventory::container::Container;
use crate::player::Player;
use crate::world::World;

/// Vanilla `BowItem.getUseDuration`.
const USE_DURATION: i32 = 72000;
/// Vanilla `BowItem.getPowerForTime` full-charge ticks.
const FULL_DRAW_TICKS: f32 = 20.0;
/// Vanilla minimum `getPowerForTime` that still fires.
const MIN_POWER: f32 = 0.1;
/// Vanilla `BowItem.releaseUsing` velocity scale (`power * 3.0`).
const PLAYER_SHOT_POWER_SCALE: f32 = 3.0;
/// Vanilla `BowItem.releaseUsing` inaccuracy.
const PLAYER_SHOT_UNCERTAINTY: f32 = 1.0;

/// Behavior for the bow item.
#[item_behavior(class = "BowItem")]
pub struct BowItem;

impl BowItem {
    /// Vanilla `BowItem.getPowerForTime`.
    #[must_use]
    pub fn power_for_time(charge: i32) -> f32 {
        let mut power = charge as f32 / FULL_DRAW_TICKS;
        power = (power * power + power * 2.0) / 3.0;
        power.min(1.0)
    }
}

impl ItemBehavior for BowItem {
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        if !has_ammo(context.player) {
            return InteractionResult::Fail;
        }
        context.player.start_using_item(context.hand);
        InteractionResult::Consume
    }

    fn get_use_animation(&self, _stack: &ItemStack) -> ItemUseAnimation {
        ItemUseAnimation::Bow
    }

    fn get_use_duration(&self, _stack: &ItemStack, _user: &dyn LivingEntity) -> i32 {
        USE_DURATION
    }

    fn release_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: &dyn LivingEntity,
        time_left: i32,
    ) -> bool {
        let charge = USE_DURATION - time_left;
        let power = Self::power_for_time(charge);
        if power < MIN_POWER {
            return false;
        }

        let Some(player) = user.as_player() else {
            return false;
        };
        if !has_ammo(player) {
            return false;
        }

        let Some(_arrow) = spawn_arrow_projectile(
            world,
            user,
            power * PLAYER_SHOT_POWER_SCALE,
            PLAYER_SHOT_UNCERTAINTY,
            ArrowEntity::DEFAULT_DAMAGE,
        ) else {
            return false;
        };

        let pitch = 1.0 / (rand::random::<f32>() * 0.4 + 1.2) + power * 0.5;
        world.play_sound_at(
            &sound_events::ENTITY_ARROW_SHOOT,
            SoundSource::Players,
            user.position(),
            1.0,
            pitch,
            None,
        );

        player.award_stat(&vanilla_stat_types::ITEM_USED, stack.item());
        if !player.has_infinite_materials() {
            stack.hurt_and_break(1, false);
            consume_ammo(player, stack);
        }
        false
    }
}

fn is_arrow_item(stack: &ItemStack) -> bool {
    REGISTRY.items.is_in_tag(stack.item(), &ItemTag::ARROWS)
}

fn has_ammo(player: &Player) -> bool {
    if player.has_infinite_materials() {
        return true;
    }
    let inventory = player.inventory.lock();
    if is_arrow_item(inventory.get_item_in_hand(steel_utils::types::InteractionHand::OffHand)) {
        return true;
    }
    inventory.get_items().iter().any(is_arrow_item)
}

fn consume_ammo(player: &Player, bow: &ItemStack) {
    if bow.get_enchantment_level(&vanilla_enchantments::INFINITY.key) > 0 {
        return;
    }
    let mut inventory = player.inventory.lock();
    let offhand = inventory.get_offhand_item();
    if is_arrow_item(offhand) {
        inventory.get_offhand_item_mut().shrink(1);
        return;
    }
    if let Some(slot) = inventory.get_items().iter().position(is_arrow_item) {
        inventory.get_item_mut(slot).shrink(1);
        Container::set_changed(&mut *inventory);
    }
}

#[cfg(test)]
mod tests {
    use super::BowItem;

    #[test]
    fn power_is_full_after_one_second() {
        assert!((BowItem::power_for_time(20) - 1.0).abs() < 1.0e-5);
        assert!((BowItem::power_for_time(40) - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn short_draw_is_too_weak_to_fire() {
        assert!(BowItem::power_for_time(2) < 0.1);
        assert!(BowItem::power_for_time(10) > 0.1);
    }
}
