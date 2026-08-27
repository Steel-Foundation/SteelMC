//! Armor stand item behavior (`ArmorStandItem`).
//!
//! Right-clicking a block places an armor stand at the placement position,
//! snapped to 45-degree yaw, matching vanilla `ArmorStandItem.useOn`.

use std::sync::Arc;

use glam::DVec3;
use steel_macros::item_behavior;
use steel_protocol::packets::game::SoundSource;
use steel_registry::blocks::properties::Direction;
use steel_registry::entity_type::EntityTypeRef;
use steel_registry::{sound_events, vanilla_entities, vanilla_game_events};
use steel_utils::WorldAabb;
use steel_utils::axis::Axis;
use steel_utils::wrap_degrees;

use crate::behavior::BlockCollisionContext;
use crate::behavior::context::{InteractionResult, UseOnContext};
use crate::behavior::item::ItemBehavior;
use crate::entity::entities::ArmorStandEntity;
use crate::entity::{Entity, LivingEntity, SharedEntity, next_entity_id};
use crate::physics::{CollisionWorld, WorldCollisionProvider, collide};
use crate::world::World;
use crate::world::game_event::GameEventContext;

/// Vanilla `ArmorStandItem` place sound volume.
const PLACE_SOUND_VOLUME: f32 = 0.75;
/// Vanilla `ArmorStandItem` place sound pitch.
const PLACE_SOUND_PITCH: f32 = 0.8;

/// Behavior for the armor stand item.
#[item_behavior(class = "ArmorStandItem")]
pub struct ArmorStandItem;

impl ArmorStandItem {
    /// Vanilla `ArmorStandItem` yaw snap: 45-degree increments facing away from the player.
    #[must_use]
    pub fn placement_yaw(player_yaw: f32) -> f32 {
        let wrapped = wrap_degrees(player_yaw - 180.0);
        ((wrapped + 22.5) / 45.0).floor() * 45.0
    }

    fn spawn_box(entity_type: EntityTypeRef, position: DVec3) -> WorldAabb {
        let dimensions = entity_type.dimensions;
        WorldAabb::entity_box(
            position.x,
            position.y,
            position.z,
            f64::from(dimensions.half_width()),
            f64::from(dimensions.height),
        )
    }

    fn spawn_y_offset(
        world: &Arc<World>,
        spawn_pos: steel_utils::BlockPos,
        entity_box: WorldAabb,
    ) -> f64 {
        let mut search = WorldAabb::new(
            f64::from(spawn_pos.x()),
            f64::from(spawn_pos.y()),
            f64::from(spawn_pos.z()),
            f64::from(spawn_pos.x()) + 1.0,
            f64::from(spawn_pos.y()) + 1.0,
            f64::from(spawn_pos.z()) + 1.0,
        );
        search = search.expand_towards(DVec3::new(0.0, -1.0, 0.0));
        let collisions = WorldCollisionProvider::new(world)
            .get_collisions_with_context(&search, BlockCollisionContext::empty());
        1.0 + collide(Axis::Y, &entity_box, &collisions, -2.0)
    }

    fn can_place(world: &Arc<World>, spawn_box: &WorldAabb) -> bool {
        let collision_world = WorldCollisionProvider::new(world);
        !collision_world.has_collision_with_context(spawn_box, BlockCollisionContext::empty())
            && world.get_entities_in_aabb(spawn_box).is_empty()
    }
}

impl ItemBehavior for ArmorStandItem {
    fn use_on(&self, context: &mut UseOnContext) -> InteractionResult {
        if context.hit_result.direction == Direction::Down {
            return InteractionResult::Fail;
        }

        let place_pos = context.build_place_context().place_pos();
        let bottom_center = DVec3::new(
            f64::from(place_pos.x()) + 0.5,
            f64::from(place_pos.y()),
            f64::from(place_pos.z()) + 0.5,
        );
        let spawn_box = Self::spawn_box(&vanilla_entities::ARMOR_STAND, bottom_center);
        if !Self::can_place(context.world, &spawn_box) {
            return InteractionResult::Fail;
        }

        let raised_box = Self::spawn_box(
            &vanilla_entities::ARMOR_STAND,
            DVec3::new(bottom_center.x, bottom_center.y + 1.0, bottom_center.z),
        );
        let y =
            f64::from(place_pos.y()) + Self::spawn_y_offset(context.world, place_pos, raised_box);
        let position = DVec3::new(bottom_center.x, y, bottom_center.z);
        let yaw = Self::placement_yaw(context.player.rotation().0);
        let stand = ArmorStandEntity::new(
            &vanilla_entities::ARMOR_STAND,
            next_entity_id(),
            position,
            Arc::downgrade(context.world),
        );
        stand.set_rotation((yaw, 0.0));
        stand.set_y_body_rot(yaw);
        stand.set_y_head_rot(yaw);
        context.inv.with_item(|item| {
            stand.apply_components_from_item_stack(item);
        });

        let entity: SharedEntity = Arc::new(stand);
        if let Err(error) = context.world.try_add_entity(Arc::clone(&entity)) {
            log::debug!("failed to spawn armor stand: {error}");
            return InteractionResult::Fail;
        }

        context.world.play_sound_at(
            &sound_events::ENTITY_ARMOR_STAND_PLACE,
            SoundSource::Blocks,
            entity.position(),
            PLACE_SOUND_VOLUME,
            PLACE_SOUND_PITCH,
            None,
        );
        context.world.game_event_at(
            &vanilla_game_events::ENTITY_PLACE,
            entity.position(),
            &GameEventContext::new(Some(context.player), None),
        );
        context.inv.with_item(|item| {
            item.shrink(1);
        });

        InteractionResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::ArmorStandItem;

    #[test]
    fn placement_yaw_snaps_to_vanilla_45_degree_increments() {
        assert_eq!(
            ArmorStandItem::placement_yaw(0.0).to_bits(),
            (-180.0_f32).to_bits()
        );
        assert_eq!(
            ArmorStandItem::placement_yaw(180.0).to_bits(),
            0.0_f32.to_bits()
        );
        assert_eq!(
            ArmorStandItem::placement_yaw(22.0).to_bits(),
            (-180.0_f32).to_bits()
        );
        assert_eq!(
            ArmorStandItem::placement_yaw(23.0).to_bits(),
            (-135.0_f32).to_bits()
        );
    }
}
