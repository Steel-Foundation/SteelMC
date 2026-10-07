//! Shared vanilla entity tick helpers.

use std::sync::Arc;

use smallvec::SmallVec;

use super::{Entity, SharedEntity};

/// Snapshots vanilla old position and rotation before an entity tick.
pub(crate) fn snapshot_old_pos_and_rot_for_tick(entity: &dyn Entity) {
    entity.set_old_position_to_current();
    entity.base().set_old_rotation_to_current();
}

/// Recursively ticks vehicle passengers that are eligible in the caller's tick context.
///
/// Mirrors vanilla `ServerLevel.tickPassenger`: invalid vehicle links are detached, and
/// passengers only recurse when the server-level entity tick list says they may tick.
pub(crate) fn tick_vehicle_passengers_if(
    vehicle: &dyn Entity,
    post_tick: &mut impl FnMut(&SharedEntity),
    can_tick: &mut impl FnMut(&SharedEntity) -> bool,
) {
    let passengers = vehicle.passengers();
    if passengers.is_empty() {
        return;
    }

    let mut visited = SmallVec::<[i32; 8]>::new();
    visited.push(vehicle.id());

    for passenger in passengers {
        tick_passenger(vehicle, &passenger, post_tick, can_tick, &mut visited);
    }
}

fn tick_passenger(
    vehicle: &dyn Entity,
    entity: &SharedEntity,
    post_tick: &mut impl FnMut(&SharedEntity),
    can_tick: &mut impl FnMut(&SharedEntity) -> bool,
    visited: &mut SmallVec<[i32; 8]>,
) {
    let entity_id = entity.id();
    assert!(
        !visited.contains(&entity_id),
        "cyclic passenger relationship involving entity {entity_id}"
    );
    visited.push(entity_id);

    if entity.is_removed()
        || entity
            .vehicle()
            .is_none_or(|current_vehicle| current_vehicle.id() != vehicle.id())
    {
        entity.stop_riding();
        let popped = visited.pop();
        debug_assert_eq!(popped, Some(entity_id));
        return;
    }

    if can_tick(entity) {
        snapshot_old_pos_and_rot_for_tick(entity.as_ref());
        entity.advance_tick_count();
        Arc::clone(entity).ride_tick();
        post_tick(entity);

        for passenger in entity.passengers() {
            tick_passenger(entity.as_ref(), &passenger, post_tick, can_tick, visited);
        }
    }

    let popped = visited.pop();
    debug_assert_eq!(popped, Some(entity_id));
}
