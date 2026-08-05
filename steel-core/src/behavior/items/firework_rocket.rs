//! Firework rocket item behavior (`FireworkRocketItem`).
//!
//! Rockets can be placed against a block face or used while fall flying to
//! attach a boosting rocket to the player.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::{sound_events, vanilla_entities};
use steel_utils::Direction;

use crate::behavior::context::{InteractionResult, UseItemContext, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::enchantment_helper;
use crate::entity::entities::FireworkRocketEntity;
use crate::entity::{Entity, Projectile, SharedEntity, next_entity_id};
use crate::world::World;

const ROCKET_PLACEMENT_OFFSET: f64 = 0.15;

/// Behavior for the firework rocket item.
#[item_behavior(class = "FireworkRocketItem")]
pub struct FireworkRocketItem;

impl FireworkRocketItem {
    fn add_rocket(world: &Arc<World>, rocket: FireworkRocketEntity) -> SharedEntity {
        let entity: SharedEntity = Arc::new(rocket);
        if let Err(error) = world.try_add_entity(Arc::clone(&entity)) {
            log::debug!("failed to spawn firework rocket: {error}");
        }
        entity
    }

    fn placement_position(click_location: DVec3, direction: Direction) -> DVec3 {
        let (step_x, step_y, step_z) = direction.offset();
        click_location
            + DVec3::new(
                f64::from(step_x) * ROCKET_PLACEMENT_OFFSET,
                f64::from(step_y) * ROCKET_PLACEMENT_OFFSET,
                f64::from(step_z) * ROCKET_PLACEMENT_OFFSET,
            )
    }
}

impl ItemBehavior for FireworkRocketItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if context.player.is_fall_flying() {
            return InteractionResult::Pass;
        }

        let source_item = context.inv.with_item(|item| item.clone());
        let position =
            Self::placement_position(context.hit_result.location, context.hit_result.direction);
        let rocket = FireworkRocketEntity::launched(
            &vanilla_entities::FIREWORK_ROCKET,
            next_entity_id(),
            position,
            Arc::downgrade(context.world),
            source_item,
        );
        rocket.set_owner_uuid(Some(context.player.uuid()));
        let rocket = Self::add_rocket(context.world, rocket);
        context.inv.with_item(|item| {
            enchantment_helper::on_projectile_spawned(
                context.world,
                item,
                rocket.as_ref(),
                Some(context.player),
            );
            item.shrink(1);
        });

        InteractionResult::Success
    }

    fn use_item(&self, context: &mut UseItemContext) -> InteractionResult {
        if !context.player.is_fall_flying() {
            return InteractionResult::Pass;
        }

        if context.player.drop_all_leash_connections(None) {
            context.world.play_sound_at(
                &sound_events::ITEM_LEAD_BREAK,
                SoundSource::Neutral,
                context.player.position(),
                1.0,
                1.0,
                None,
            );
        }

        let source_item = context.inv.with_item(|item| item.clone());
        let rocket = FireworkRocketEntity::attached_to_living(
            &vanilla_entities::FIREWORK_ROCKET,
            next_entity_id(),
            Arc::downgrade(context.world),
            source_item,
            context.player,
        );
        let rocket = Self::add_rocket(context.world, rocket);
        context.inv.with_item(|item| {
            enchantment_helper::on_projectile_spawned(
                context.world,
                item,
                rocket.as_ref(),
                Some(context.player),
            );
            item.shrink(1);
        });
        // TODO: Award `Stats.ITEM_USED` once Steel has a statistics foundation.

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_position_offsets_outside_clicked_face() {
        let click = DVec3::new(1.25, 2.5, 3.75);

        assert_eq!(
            FireworkRocketItem::placement_position(click, Direction::Up),
            DVec3::new(1.25, 2.65, 3.75)
        );
        assert_eq!(
            FireworkRocketItem::placement_position(click, Direction::West),
            DVec3::new(1.1, 2.5, 3.75)
        );
    }
}
