//! Shared vanilla entity tick helpers.

use rustc_hash::FxHashSet;

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
pub(crate) fn tick_vehicle_passengers_with_ticked_if(
    vehicle: &dyn Entity,
    ticked_entities: &mut FxHashSet<i32>,
    post_tick: &mut impl FnMut(&SharedEntity),
    can_tick: &mut impl FnMut(&SharedEntity) -> bool,
) {
    let mut visited = FxHashSet::default();
    visited.insert(vehicle.id());

    for passenger in vehicle.passengers() {
        tick_passenger(
            vehicle,
            &passenger,
            ticked_entities,
            post_tick,
            can_tick,
            &mut visited,
        );
    }
}

fn tick_passenger(
    vehicle: &dyn Entity,
    entity: &SharedEntity,
    ticked_entities: &mut FxHashSet<i32>,
    post_tick: &mut impl FnMut(&SharedEntity),
    can_tick: &mut impl FnMut(&SharedEntity) -> bool,
    visited: &mut FxHashSet<i32>,
) {
    assert!(
        visited.insert(entity.id()),
        "cyclic passenger relationship involving entity {}",
        entity.id()
    );

    if entity.is_removed()
        || entity
            .vehicle()
            .is_none_or(|current_vehicle| current_vehicle.id() != vehicle.id())
    {
        entity.stop_riding();
        visited.remove(&entity.id());
        return;
    }

    if can_tick(entity) && ticked_entities.insert(entity.id()) {
        snapshot_old_pos_and_rot_for_tick(entity.as_ref());
        entity.advance_tick_count();
        entity.ride_tick();
        post_tick(entity);

        for passenger in entity.passengers() {
            tick_passenger(
                entity.as_ref(),
                &passenger,
                ticked_entities,
                post_tick,
                can_tick,
                visited,
            );
        }
    }

    visited.remove(&entity.id());
}
