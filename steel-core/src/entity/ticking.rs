//! Shared vanilla entity tick helpers.

use rustc_hash::FxHashSet;

use super::{Entity, SharedEntity};

/// Recursively ticks direct and indirect passengers for a non-passenger vehicle.
///
/// Mirrors vanilla `ServerLevel.tickPassenger`.
pub(crate) fn tick_vehicle_passengers(
    vehicle: &dyn Entity,
    post_tick: &mut impl FnMut(&SharedEntity),
) {
    let mut ticked_entities = FxHashSet::default();
    ticked_entities.insert(vehicle.id());
    tick_vehicle_passengers_with_ticked(vehicle, &mut ticked_entities, post_tick);
}

/// Recursively ticks vehicle passengers using the caller's per-tick scheduler state.
pub(crate) fn tick_vehicle_passengers_with_ticked(
    vehicle: &dyn Entity,
    ticked_entities: &mut FxHashSet<i32>,
    post_tick: &mut impl FnMut(&SharedEntity),
) {
    let mut visited = FxHashSet::default();
    visited.insert(vehicle.id());

    for passenger in vehicle.passengers() {
        tick_passenger(
            vehicle,
            &passenger,
            ticked_entities,
            post_tick,
            &mut visited,
        );
    }
}

fn tick_passenger(
    vehicle: &dyn Entity,
    entity: &SharedEntity,
    ticked_entities: &mut FxHashSet<i32>,
    post_tick: &mut impl FnMut(&SharedEntity),
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

    if ticked_entities.insert(entity.id()) {
        entity.advance_tick_count();
        entity.ride_tick();
        post_tick(entity);

        for passenger in entity.passengers() {
            tick_passenger(
                entity.as_ref(),
                &passenger,
                ticked_entities,
                post_tick,
                visited,
            );
        }
    }

    visited.remove(&entity.id());
}
