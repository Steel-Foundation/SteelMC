//! Shared vanilla entity tick helpers.

use rustc_hash::FxHashSet;

use super::{Entity, SharedEntity};

/// Recursively ticks direct and indirect passengers for a non-passenger vehicle.
///
/// Mirrors vanilla `ServerLevel.tickPassenger`.
pub(crate) fn tick_vehicle_passengers(
    vehicle: &dyn Entity,
    server_tick: i32,
    post_tick: &mut impl FnMut(&SharedEntity),
) {
    let mut visited = FxHashSet::default();
    visited.insert(vehicle.id());

    for passenger in vehicle.passengers() {
        tick_passenger(vehicle, &passenger, server_tick, post_tick, &mut visited);
    }
}

fn tick_passenger(
    vehicle: &dyn Entity,
    entity: &SharedEntity,
    server_tick: i32,
    post_tick: &mut impl FnMut(&SharedEntity),
    visited: &mut FxHashSet<i32>,
) {
    if !visited.insert(entity.id()) {
        panic!(
            "cyclic passenger relationship involving entity {}",
            entity.id()
        );
    }

    if entity.is_removed()
        || entity
            .vehicle()
            .is_none_or(|current_vehicle| current_vehicle.id() != vehicle.id())
    {
        entity.stop_riding();
        visited.remove(&entity.id());
        return;
    }

    if !entity.was_ticked_this_tick(server_tick) {
        entity.mark_ticked(server_tick);
        entity.advance_tick_count();
        entity.ride_tick();
        post_tick(entity);

        for passenger in entity.passengers() {
            tick_passenger(entity.as_ref(), &passenger, server_tick, post_tick, visited);
        }
    }

    visited.remove(&entity.id());
}
