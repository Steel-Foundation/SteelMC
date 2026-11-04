use glam::DVec3;

use super::EntityPush;
use crate::entity::{Entity, LivingEntity};

/// Vanilla push and cramming behavior with no overrides. Implementors only read state,
/// only write velocity and the velocity sync flag, and never re-enter the entity manager.
/// Entities that override contact, velocity or cramming hooks must not opt in.
pub trait DefaultEntityPush: Entity {
    /// Push inputs for one contact, or `None` while riding. `known_pushable` skips the
    /// `is_pushable` check when the selector has already passed this entity.
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

/// Saved inputs for one push contact.
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
