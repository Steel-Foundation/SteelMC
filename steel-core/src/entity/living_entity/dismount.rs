//! Leaving a vehicle.

use steel_registry::vanilla_block_tags::BlockTag;
use steel_utils::WorldAabb;

use super::*;
use crate::physics::WorldCollisionProvider;

/// Entities at most this wide and tall look for free space when their
/// vehicle is gone or in a portal.
const SMALL_ENTITY_MAX_SIZE: f32 = 4.0;

impl Living<'_> {
    #[expect(clippy::unused_self, reason = "the hook default for every living type")]
    pub(super) const fn dismount_poses_default(&self) -> &'static [EntityPose] {
        &[EntityPose::Standing]
    }

    /// Vanilla `LivingEntity.dismountVehicle`: moves this entity off
    /// `vehicle`, which it just stopped riding.
    pub fn dismount_vehicle(&self, vehicle: &dyn Entity) {
        let Some(world) = self.level() else {
            return;
        };
        let target = if self.is_removed() {
            self.position()
        } else if !vehicle.is_removed()
            && !world
                .get_block_state(vehicle.block_position())
                .get_block()
                .has_tag(&BlockTag::PORTALS)
        {
            vehicle.dismount_location_for_passenger(self)
        } else {
            self.free_dismount_position(&world, vehicle)
        };

        if let Err(error) = self.teleport_to(target) {
            log::warn!("failed to dismount entity {}: {error}", self.id());
        }
    }

    /// Above the higher of this entity and `vehicle`, nudged into free space
    /// for small entities.
    fn free_dismount_position(&self, world: &Arc<World>, vehicle: &dyn Entity) -> DVec3 {
        let position = self.position();
        let target = DVec3::new(position.x, position.y.max(vehicle.position().y), position.z);
        let dimensions = self.base().dimensions();
        if dimensions.width > SMALL_ENTITY_MAX_SIZE || dimensions.height > SMALL_ENTITY_MAX_SIZE {
            return target;
        }

        let (width, height) = (f64::from(dimensions.width), f64::from(dimensions.height));
        let half_height = height / 2.0;
        let center = target + DVec3::new(0.0, half_height, 0.0);
        let allowed_centers = [WorldAabb::of_size(center, width, height, width)];
        WorldCollisionProvider::for_entity(world, self.entity())
            .find_free_position(&allowed_centers, center, width, height, width)
            .map_or(target, |free| free - DVec3::new(0.0, half_height, 0.0))
    }
}
