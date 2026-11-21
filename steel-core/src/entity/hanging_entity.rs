use std::ptr;
use steel_utils::{Direction, WorldAabb};

use super::{Entity, physics_state_for_move};
use crate::physics::{CollisionWorld, WorldCollisionProvider};

/// Attachment geometry shared by item frames and paintings.
pub trait HangingEntity: Entity {
    /// Direction from the supporting block toward this attachment.
    fn direction(&self) -> Direction;

    /// Bounds used for support and coexistence, independently of map hit bounds.
    fn pop_box(&self) -> WorldAabb;

    /// Checks blocks and the world border using this entity's collision context.
    fn has_level_collision(&self) -> bool {
        let Some(world) = self.level() else {
            return false;
        };
        let collisions = WorldCollisionProvider::for_entity(&world, self.as_entity_event_source());
        let bounds = self.pop_box();
        collisions.has_block_collision_with_context(
            &bounds,
            physics_state_for_move(self.as_entity_event_source()).block_collision_context(),
        ) || collisions.has_world_border_collision(&bounds)
    }

    /// Allows perpendicular frames to share attachment space, as in vanilla.
    fn can_coexist(&self, allow_intersecting_same_type: bool) -> bool {
        let Some(world) = self.level() else {
            return true;
        };
        !world.has_entity_in_aabb_matching(&self.pop_box(), |entity| {
            entity.as_hanging_entity().is_some_and(|other| {
                !ptr::eq(entity, self.as_entity_event_source())
                    && ((!allow_intersecting_same_type
                        && entity.entity_type() == self.entity_type())
                        || other.direction() == self.direction())
            })
        })
    }
}
