use glam::DVec3;
use steel_utils::{BlockPos, WorldAabb, locks::SyncMutex};

use crate::{entity::ContainerUser, inventory::lock::ContainerId, world::World};

#[derive(Default)]
struct OpenersState {
    count: i32,
    max_interaction_range: f64,
}

pub(crate) struct OpenersChange {
    pub(crate) previous: i32,
    pub(crate) current: i32,
}

/// Counts container users without holding the counter lock during world callbacks.
#[derive(Default)]
pub(crate) struct ContainerOpenersCounter {
    state: SyncMutex<OpenersState>,
}

impl ContainerOpenersCounter {
    pub(crate) fn increment(&self, user: &dyn ContainerUser) -> OpenersChange {
        let interaction_range = user.container_interaction_range();
        let mut state = self.state.lock();
        let previous = state.count;
        state.count += 1;
        state.max_interaction_range = state.max_interaction_range.max(interaction_range);
        OpenersChange {
            previous,
            current: state.count,
        }
    }

    pub(crate) fn decrement(&self) -> OpenersChange {
        let mut state = self.state.lock();
        let previous = state.count;
        state.count -= 1;
        if state.count == 0 {
            state.max_interaction_range = 0.0;
        }
        OpenersChange {
            previous,
            current: state.count,
        }
    }

    pub(crate) fn recheck(
        &self,
        world: &World,
        pos: BlockPos,
        container: ContainerId,
    ) -> OpenersChange {
        let range = self.state.lock().max_interaction_range + 4.0;
        let min = DVec3::new(f64::from(pos.x()), f64::from(pos.y()), f64::from(pos.z()));
        let bounds = WorldAabb::new(min.x, min.y, min.z, min.x + 1.0, min.y + 1.0, min.z + 1.0)
            .inflate(range);
        let mut count = 0;
        let mut max_range: f64 = 0.0;
        for entity in world.get_entities_in_aabb(&bounds) {
            let Some(user) = entity.as_container_user() else {
                continue;
            };
            if !user.is_spectator() && user.has_container_open_for(container) {
                count += 1;
                max_range = max_range.max(user.container_interaction_range());
            }
        }
        let mut state = self.state.lock();
        let previous = state.count;
        state.count = count;
        state.max_interaction_range = max_range;
        OpenersChange {
            previous,
            current: count,
        }
    }
}

#[cfg(test)]
mod tests;
