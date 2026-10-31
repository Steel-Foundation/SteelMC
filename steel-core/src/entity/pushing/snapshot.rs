use glam::DVec3;

use super::EntityPush;
use crate::entity::{Entity, LivingEntity};

/// base push and cramming behavior, pure reads, only velocity + sync writes
/// no manager reentry; custom contact/velocity/cramming hooks keep `None`
pub trait DefaultEntityPush: Entity {
    /// riding returns `None`; known pushable needs selection without cramming effects
    fn push_snapshot(&self, known_pushable: bool) -> Option<EntityPushSnapshot> {
        let state = self.base().unmounted_push_state()?;

        Some(EntityPushSnapshot {
            position: state.position,
            no_physics: state.no_physics,
            receives_impulse: !state.is_vehicle && (known_pushable || self.is_pushable()),
            sleeping: self
                .as_living_entity()
                .is_some_and(LivingEntity::is_sleeping),
        })
    }
}

/// saved inputs for one push contact
#[derive(Clone, Copy, Debug)]
pub struct EntityPushSnapshot {
    position: DVec3,
    no_physics: bool,
    receives_impulse: bool,
    sleeping: bool,
}

impl EntityPush {
    pub(super) fn apply_contact(
        source: &dyn Entity,
        source_state: EntityPushSnapshot,
        target: &dyn Entity,
        target_state: EntityPushSnapshot,
    ) {
        if source_state.sleeping || source_state.no_physics || target_state.no_physics {
            return;
        }

        let Some(impulse) = Self::impulse(source_state.position, target_state.position) else {
            return;
        };

        if source_state.receives_impulse {
            source
                .base()
                .push_impulse(DVec3::new(-impulse.x, 0.0, -impulse.z));
        }

        if target_state.receives_impulse {
            target.base().push_impulse(impulse);
        }
    }
}
