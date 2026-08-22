//! Bow item behavior (`net.minecraft.world.item.BowItem`).
//!
//! Holding use draws the bow; releasing fires an [`ArrowEntity`] along the
//! look direction with vanilla's draw-power curve (`((t/20)^2 + 2t/20)/3`,
//! clamped to 1). Draws under 0.1 power abort without consuming ammo. One
//! arrow is consumed per shot unless the player has infinite materials.
//!
//! Not implemented yet (missing foundations): enchantment modifiers
//! (Power/Punch/Flame).

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::item_stack::ItemStack;
use steel_registry::sound_events;
use steel_registry::vanilla_entities;
use steel_registry::vanilla_item_tags::ItemTag;

use crate::behavior::context::{InteractionResult, UseItemContext};
use crate::behavior::{ItemBehavior, ItemUseAnimation};
use crate::entity::entities::{ArrowEntity, Pickup};
use crate::entity::{Entity, LivingEntity, Projectile, SharedEntity, next_entity_id};
use crate::inventory::prelude::Container;
use crate::world::World;

/// Vanilla `Item.getUseDuration` for bows (draw is released manually).
const USE_DURATION: i32 = 72000;
/// Vanilla `BowItem.MAX_DRAW_DURATION`.
const MAX_DRAW_DURATION: i32 = 20;
/// Vanilla `BowItem.releaseUsing`: draws under this power abort unfired.
const MIN_RELEASE_POWER: f32 = 0.1;
/// Vanilla `BowItem.releaseUsing`: `pow == 1.0F` marks the shot as a crit.
/// [`Self::get_power_for_time`] clamps at this value, so `>=` is equivalent.
const FULL_DRAW_POWER: f32 = 1.0;

/// Behavior for the bow item.
#[item_behavior(class = "BowItem")]
pub struct BowItem;

impl ItemBehavior for BowItem {
    /// Vanilla `BowItem.use`: start drawing when ammo is available.
    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        let has_ammo = context.inv.with_inventory(|inventory| {
            Container::items(inventory)
                .iter()
                .any(|stack| !stack.is_empty() && stack.item().has_tag(&ItemTag::ARROWS))
        }) || context.player.has_infinite_materials();
        if !has_ammo {
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

    /// Vanilla `BowItem.releaseUsing`: fire the drawn arrow.
    fn release_using(
        &self,
        stack: &mut ItemStack,
        world: &Arc<World>,
        user: &dyn LivingEntity,
        time_left: i32,
    ) -> bool {
        let Some(player) = user.as_player() else {
            return false;
        };

        let held_ticks = USE_DURATION.saturating_sub(time_left);
        let power = Self::get_power_for_time(held_ticks);
        if power < MIN_RELEASE_POWER {
            return false;
        }

        let ammo_slot = {
            let inventory = player.inventory.lock();
            inventory.items().iter().position(|candidate| {
                !candidate.is_empty() && candidate.item().has_tag(&ItemTag::ARROWS)
            })
        };
        if ammo_slot.is_none() && !player.has_infinite_materials() {
            return false;
        }

        if let Some(slot) = ammo_slot {
            {
                let mut inventory = player.inventory.lock();
                inventory.items_mut()[slot].shrink(1);
            }
            player.request_inventory_resync([slot]);
        }

        // TODO: vanilla selects the projectile via `Player.getProjectile`
        // (Infinity, creative default arrow) and copies the consumed stack's
        // components onto the entity (`AbstractArrow` ctor), so tipped/spectral
        // arrows keep their effects. Enchantment modifiers (Power/Punch/Flame)
        // apply through `EnchantmentHelper` on release. Both need enchantment
        // + component-transfer foundations.
        let player_pos = player.position();
        let spawn_pos = DVec3::new(player_pos.x, player.get_eye_y() - 0.1, player_pos.z);
        let arrow = Arc::new(ArrowEntity::new(
            &vanilla_entities::ARROW,
            next_entity_id(),
            spawn_pos,
            Arc::downgrade(world),
        ));
        if let Some(owner) = world.players.get_by_uuid(&player.gameprofile.id) {
            let owner: SharedEntity = owner;
            arrow.set_owner_entity(Some(&owner));
        } else {
            arrow.set_owner_uuid(Some(player.gameprofile.id));
        }
        arrow.set_pickup(Pickup::Allowed);
        if power >= FULL_DRAW_POWER {
            arrow.set_crit_arrow(true);
        }

        let (yaw, pitch) = player.rotation();
        arrow.shoot_from_rotation(user, pitch, yaw, 0.0, power * 3.0, 1.0);

        let entity: SharedEntity = arrow;
        if let Err(error) = world.try_add_entity(entity.clone()) {
            log::debug!("failed to spawn arrow: {error}");
            return false;
        }

        let sound_pitch = 1.0 / (rand::random::<f32>() * 0.4 + 1.2) + power * 0.5;
        world.play_sound_at(
            &sound_events::ENTITY_ARROW_SHOOT,
            SoundSource::Players,
            player.position(),
            1.0,
            sound_pitch,
            None,
        );
        stack.hurt_and_break(1, player.has_infinite_materials());
        // TODO: award ITEM_USED stat for bow
        true
    }
}

impl BowItem {
    /// Vanilla `BowItem.getPowerForTime`.
    fn get_power_for_time(ticks: i32) -> f32 {
        let fraction = ticks as f32 / MAX_DRAW_DURATION as f32;
        let power = (fraction * fraction + 2.0 * fraction) / 3.0;
        if power > 1.0 { 1.0 } else { power }
    }
}

#[cfg(test)]
mod tests {
    use super::BowItem;

    #[test]
    fn draw_power_matches_vanilla_curve() {
        assert!(BowItem::get_power_for_time(0).abs() < f32::EPSILON);
        // Below the 0.1 release threshold early in the draw.
        assert!(BowItem::get_power_for_time(1) < 0.1);
        // t=12: ((0.6)^2 + 1.2) / 3 = 0.52.
        assert!((BowItem::get_power_for_time(12) - 0.52).abs() < 1e-6);
        assert!((BowItem::get_power_for_time(20) - 1.0).abs() < 1e-6);
        // Clamped past full draw.
        assert!((BowItem::get_power_for_time(40) - 1.0).abs() < f32::EPSILON);
    }
}
