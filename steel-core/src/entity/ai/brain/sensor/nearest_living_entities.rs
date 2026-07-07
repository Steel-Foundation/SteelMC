//! Scans nearby living entities into `NEAREST_VISIBLE_LIVING_ENTITIES`, closest first.

use glam::DVec3;
use steel_registry::vanilla_attributes;

use super::Sensor;
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType};
use crate::entity::{PathfinderMob, SharedEntity};

pub(crate) struct NearestLivingEntitiesSensor;

impl Sensor for NearestLivingEntitiesSensor {
    fn requires(&self) -> &[MemoryModuleType] {
        &[MemoryModuleType::NearestVisibleLivingEntities]
    }

    fn do_tick(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories) {
        let Some(world) = mob.level() else {
            return;
        };
        let follow_range = mob
            .attributes()
            .lock()
            .required_value(vanilla_attributes::FOLLOW_RANGE);
        let search_box = mob
            .bounding_box()
            .inflate_xyz(follow_range, follow_range, follow_range);
        let mob_id = mob.id();

        let mut entities = world.get_entities_in_aabb_matching(&search_box, |entity| {
            entity.id() != mob_id && entity.is_alive() && entity.as_living_entity().is_some()
        });
        sort_by_distance(&mut entities, mob.position());

        memories.set_nearest_visible_living_entities(entities);
    }
}

fn sort_by_distance(entities: &mut [SharedEntity], origin: DVec3) {
    entities.sort_by(|a, b| {
        origin
            .distance_squared(a.position())
            .total_cmp(&origin.distance_squared(b.position()))
    });
}
