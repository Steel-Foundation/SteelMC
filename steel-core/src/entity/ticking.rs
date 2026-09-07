//! Shared vanilla entity tick helpers.

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
        entity.ride_tick();
        post_tick(entity);

        for passenger in entity.passengers() {
            tick_passenger(entity.as_ref(), &passenger, post_tick, can_tick, visited);
        }
    }

    let popped = visited.pop();
    debug_assert_eq!(popped, Some(entity_id));
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Weak};

    use glam::DVec3;
    use steel_registry::{entity_type::EntityTypeRef, vanilla_entities};
    use steel_utils::locks::SyncMutex;

    use super::*;
    use crate::entity::EntityBase;

    struct PassengerTickTestEntity {
        base: EntityBase,
        ride_order: Arc<SyncMutex<Vec<i32>>>,
    }

    impl PassengerTickTestEntity {
        fn shared(id: i32, ride_order: &Arc<SyncMutex<Vec<i32>>>) -> SharedEntity {
            Arc::new(Self {
                base: EntityBase::new(
                    id,
                    DVec3::ZERO,
                    vanilla_entities::ITEM.dimensions,
                    Weak::new(),
                ),
                ride_order: Arc::clone(ride_order),
            })
        }
    }

    crate::entity::impl_test_downcast_type!(PassengerTickTestEntity);

    impl Entity for PassengerTickTestEntity {
        fn base(&self) -> &EntityBase {
            &self.base
        }

        fn entity_type(&self) -> EntityTypeRef {
            &vanilla_entities::ITEM
        }

        fn ride_tick(&self) {
            self.ride_order.lock().push(self.id());
        }
    }

    #[test]
    fn deep_passenger_tree_preserves_depth_first_order_after_inline_capacity() {
        const DESCENDANT_COUNT: i32 = 10;

        let ride_order = Arc::new(SyncMutex::new(Vec::new()));
        let entities = (1..=DESCENDANT_COUNT + 1)
            .map(|id| PassengerTickTestEntity::shared(id, &ride_order))
            .collect::<Vec<_>>();
        for pair in entities.windows(2) {
            EntityBase::restore_passenger_relationship(&pair[0], &pair[1]);
        }

        let mut post_order = Vec::new();
        let mut can_tick_order = Vec::new();
        tick_vehicle_passengers_if(
            entities[0].as_ref(),
            &mut |entity| post_order.push(entity.id()),
            &mut |entity| {
                can_tick_order.push(entity.id());
                true
            },
        );

        let expected = (2..=DESCENDANT_COUNT + 1).collect::<Vec<_>>();
        assert_eq!(*ride_order.lock(), expected);
        assert_eq!(post_order, expected);
        assert_eq!(can_tick_order, expected);
        assert!(
            entities
                .iter()
                .skip(1)
                .all(|entity| entity.tick_count() == 1)
        );
    }

    #[test]
    #[should_panic(expected = "cyclic passenger relationship involving entity 1")]
    fn passenger_cycle_keeps_cycle_assertion() {
        let ride_order = Arc::new(SyncMutex::new(Vec::new()));
        let root = PassengerTickTestEntity::shared(1, &ride_order);
        let child = PassengerTickTestEntity::shared(2, &ride_order);
        EntityBase::restore_passenger_relationship(&root, &child);
        EntityBase::restore_passenger_relationship(&child, &root);

        tick_vehicle_passengers_if(root.as_ref(), &mut |_| {}, &mut |_| true);
    }
}
