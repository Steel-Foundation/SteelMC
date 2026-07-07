//! picks the nearest visible entity matching a predicate and makes it the look target.

use glam::DVec3;

use super::{Behavior, BehaviorState};
use crate::entity::ai::brain::memory::{Memories, MemoryModuleType, MemoryStatus, PositionTracker};
use crate::entity::{Entity, PathfinderMob, SharedEntity};

const ENTRY_CONDITION: &[(MemoryModuleType, MemoryStatus)] = &[
    (MemoryModuleType::LookTarget, MemoryStatus::ValueAbsent),
    (
        MemoryModuleType::NearestVisibleLivingEntities,
        MemoryStatus::ValuePresent,
    ),
];

pub(crate) struct SetEntityLookTarget {
    state: BehaviorState,
    can_look_at: Box<dyn Fn(&dyn Entity) -> bool + Send + Sync>,
    max_dist_sqr: f64,
}

impl SetEntityLookTarget {
    #[must_use]
    pub(crate) fn new(
        can_look_at: impl Fn(&dyn Entity) -> bool + Send + Sync + 'static,
        max_dist: f32,
    ) -> Self {
        Self {
            state: BehaviorState::new(ENTRY_CONDITION),
            can_look_at: Box::new(can_look_at),
            max_dist_sqr: f64::from(max_dist) * f64::from(max_dist),
        }
    }

    fn find_closest(&self, entities: &[SharedEntity], mob_position: DVec3) -> Option<SharedEntity> {
        let mut best: Option<(&SharedEntity, f64)> = None;
        for entity in entities {
            let distance_sqr = mob_position.distance_squared(entity.position());
            if distance_sqr > self.max_dist_sqr || !(self.can_look_at)(&**entity) {
                continue;
            }
            if best.is_none_or(|(_, best_distance)| distance_sqr < best_distance) {
                best = Some((entity, distance_sqr));
            }
        }
        best.map(|(entity, _)| entity.clone())
    }
}

impl Behavior for SetEntityLookTarget {
    fn state(&self) -> &BehaviorState {
        &self.state
    }

    fn state_mut(&mut self) -> &mut BehaviorState {
        &mut self.state
    }

    fn start(&mut self, mob: &dyn PathfinderMob, memories: &mut Memories, _time: i64) {
        let mob_position = mob.position();
        let target = memories
            .nearest_visible_living_entities()
            .and_then(|entities| self.find_closest(entities, mob_position));
        if let Some(entity) = target {
            memories.set_look_target(PositionTracker::Entity {
                entity,
                track_eye_height: true,
            });
        }
    }
}
